//! Direct Sign in with ChatGPT for a locally run, open-source Lexi installation.
//! Credentials never cross the CLI/GUI/Python boundary. No API-key fallback.
use crate::{config, HostError};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use fs2::FileExt;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{File, OpenOptions},
    io::Write,
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const ISSUER: &str = "https://auth.openai.com";
const TOKEN: &str = "https://auth.openai.com/api/accounts/oauth/token";
const RESOURCE: &str = "https://api.openai.com/v1";
const PLAN_SCOPE: &str = "chatgpt.tokens.use.direct";
const USAGE: &str = "https://chatgpt.com/settings/usage";

#[derive(Default, Serialize, Deserialize)]
struct Store {
    host: String,
    accounts: BTreeMap<String, Account>,
}
#[derive(Clone, Serialize, Deserialize)]
struct Account {
    client_id: String,
    subject: String,
    email: String,
    #[serde(default)]
    access_token: String,
    #[serde(default)]
    refresh_token: String,
    #[serde(default)]
    id_token: String,
    #[serde(default)]
    scope: String,
    #[serde(default)]
    expires_at: u64,
}
fn err(message: &str) -> HostError {
    HostError::Config(message.into())
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn root() -> PathBuf {
    config::lexi_home().join("chatgpt")
}
fn random() -> String {
    let mut b = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut b);
    URL_SAFE_NO_PAD.encode(b)
}
fn label(s: &str) -> Result<(), HostError> {
    if s.is_empty()
        || s.len() > 64
        || !s
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
    {
        return Err(err(
            "Account label must contain 1–64 letters, digits, hyphens or underscores",
        ));
    }
    Ok(())
}
// Separate nonblocking lock keeps login/logout from racing across app/CLI processes.
// Refreshes use the credential lock and remain available during browser sign-in.
fn account_action_lock() -> Result<File, HostError> {
    let mut options = OpenOptions::new();
    options.create(true).read(true).write(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(root().join("account-action.lock"))?;
    file.try_lock_exclusive().map_err(|_| {
        err("Another ChatGPT sign-in or sign-out is pending; finish it or wait for its timeout")
    })?;
    Ok(file)
}
async fn lock() -> Result<File, HostError> {
    std::fs::create_dir_all(root())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(root(), std::fs::Permissions::from_mode(0o700))?;
    }
    let mut opts = OpenOptions::new();
    opts.create(true).read(true).write(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let f = opts.open(root().join("lock"))?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(35);
    loop {
        match f.try_lock_exclusive() {
            Ok(()) => return Ok(f),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if tokio::time::Instant::now() > deadline {
                    return Err(err("ChatGPT credentials are busy; retry shortly"));
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            Err(e) => return Err(e.into()),
        }
    }
}
fn load() -> Result<Store, HostError> {
    match std::fs::read(root().join("accounts.json")) {
        Ok(b) => Ok(serde_json::from_slice(&b)?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Store::default()),
        Err(e) => Err(e.into()),
    }
}
fn save(s: &Store) -> Result<(), HostError> {
    let mut f = tempfile::NamedTempFile::new_in(root())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        f.as_file()
            .set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    f.write_all(&serde_json::to_vec(s)?)?;
    f.as_file().sync_all()?;
    f.persist(root().join("accounts.json"))
        .map_err(|e| e.error)?;
    Ok(())
}
fn client() -> Result<reqwest::Client, HostError> {
    Ok(reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::none())
        .build()?)
}
async fn token_request(form: &[(&str, &str)]) -> Result<Value, HostError> {
    let response = client()?.post(TOKEN).form(form).send().await?;
    if !response.status().is_success() {
        return Err(err(
            "ChatGPT token exchange failed; sign in again. No API key was used.",
        ));
    }
    Ok(response.json().await?)
}
fn apply_tokens(a: &mut Account, v: &Value, initial: bool) -> Result<(), HostError> {
    let access = v["access_token"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| err("Missing ChatGPT access token"))?;
    if v["token_type"]
        .as_str()
        .is_none_or(|s| !s.eq_ignore_ascii_case("bearer"))
    {
        return Err(err("Unsupported ChatGPT token type"));
    }
    let scope = v["scope"]
        .as_str()
        .unwrap_or(if initial { "" } else { &a.scope })
        .to_owned();
    if !scope.split_whitespace().any(|s| s == PLAN_SCOPE) {
        return Err(err(
            "ChatGPT plan permission was not granted; sign in and enable plan usage",
        ));
    }
    let expires = v["expires_in"]
        .as_u64()
        .filter(|n| *n > 0)
        .ok_or_else(|| err("Missing ChatGPT token expiry"))?;
    a.access_token = access.into();
    a.scope = scope;
    a.expires_at = now().saturating_add(expires);
    if let Some(t) = v["refresh_token"].as_str() {
        a.refresh_token = t.into();
    }
    if initial && a.refresh_token.is_empty() {
        return Err(err("Missing ChatGPT refresh token"));
    }
    Ok(())
}

/// List saved registrations without exposing tokens. Each label maps to a separate provider.
pub async fn chatgpt_accounts() -> Result<Value, HostError> {
    let _guard = lock().await?;
    let s = load()?;
    Ok(json!(s.accounts.iter().map(|(label,a)|json!({"label":label,"email":a.email,"client_id":a.client_id,"signed_in":!a.access_token.is_empty(),"provider":format!("chatgpt-{label}"),"usage_url":USAGE})).collect::<Vec<_>>()))
}

#[derive(Clone, Deserialize)]
struct Claims {
    sub: String,
    nonce: String,
    #[serde(default)]
    email: String,
}
async fn validate_id(token: &str, client_id: &str, nonce: &str) -> Result<Claims, HostError> {
    use jsonwebtoken::{decode_header, jwk::JwkSet, Algorithm, DecodingKey};
    let header = decode_header(token).map_err(|_| err("Invalid ChatGPT ID token"))?;
    if header.alg != Algorithm::RS256 {
        return Err(err("Unexpected ChatGPT ID-token algorithm"));
    }
    let keys: JwkSet = client()?
        .get(format!("{ISSUER}/.well-known/jwks.json"))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let key = header
        .kid
        .as_deref()
        .and_then(|id| keys.find(id))
        .ok_or_else(|| err("Unknown ChatGPT signing key"))?;
    verify_identity(
        token,
        client_id,
        nonce,
        &DecodingKey::from_jwk(key).map_err(|_| err("Invalid ChatGPT signing key"))?,
    )
}
fn verify_identity(
    token: &str,
    client_id: &str,
    nonce: &str,
    key: &jsonwebtoken::DecodingKey,
) -> Result<Claims, HostError> {
    use jsonwebtoken::{decode, Algorithm, Validation};
    let mut validation = Validation::new(Algorithm::RS256);
    validation.set_issuer(&[ISSUER]);
    validation.set_audience(&[client_id]);
    validation.set_required_spec_claims(&["exp", "iss", "aud", "sub"]);
    let claims = decode::<Claims>(token, key, &validation)
        .map_err(|_| err("ChatGPT identity validation failed"))?
        .claims;
    if claims.nonce != nonce || claims.sub.is_empty() {
        return Err(err("ChatGPT sign-in nonce or identity did not match"));
    }
    Ok(claims)
}
fn callback(
    query: &str,
    state: &str,
    existing: Option<&str>,
) -> Result<(String, String), HostError> {
    let u = reqwest::Url::parse(&format!("http://127.0.0.1{query}"))
        .map_err(|_| err("Invalid sign-in callback"))?;
    if u.path() != "/auth/callback" {
        return Err(err("Invalid sign-in callback path"));
    }
    let mut p = BTreeMap::new();
    for (k, v) in u.query_pairs() {
        if p.insert(k.into_owned(), v.into_owned()).is_some() {
            return Err(err("Duplicate sign-in callback parameter"));
        }
    }
    if p.get("state").map(String::as_str) != Some(state) {
        return Err(err("Sign-in state did not match"));
    }
    if p.contains_key("error") {
        return Err(err("ChatGPT sign-in was declined or failed"));
    }
    let cid = p
        .get("client_id")
        .map(String::as_str)
        .or(existing)
        .filter(|s| s.starts_with("oaiapp_"))
        .ok_or_else(|| err("Missing issued ChatGPT client ID"))?;
    if existing.is_some_and(|s| s != cid) {
        return Err(err("ChatGPT registration changed during sign-in"));
    }
    let code = p
        .get("code")
        .filter(|s| !s.is_empty())
        .ok_or_else(|| err("Missing authorization code"))?;
    Ok((code.clone(), cid.into()))
}
/// Open the system browser, validate the loopback callback, and save a registration.
/// A label reuses its issued client ID on subsequent sign-ins. A new label adds an account.
pub async fn chatgpt_login(account: &str) -> Result<Value, HostError> {
    label(account)?;
    {
        let _guard = lock().await?;
    } // initialize the protected directory
    let _action = account_action_lock()?;
    let (host, previous) = {
        let _guard = lock().await?;
        let mut s = load()?;
        if s.host.is_empty() {
            s.host = format!("urn:uuid:{}", uuid::Uuid::new_v4());
            save(&s)?;
        }
        (s.host, s.accounts.get(account).cloned())
    };
    let existing_provider = config::load_providers()?;
    if let Some(existing) = existing_provider["providers"].get(format!("chatgpt-{account}")) {
        if existing["chatgpt_account"].as_str() != Some(account) {
            return Err(err(
                "That provider name already exists; choose a different ChatGPT account label",
            ));
        }
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let redirect = format!(
        "http://127.0.0.1:{}/auth/callback",
        listener.local_addr()?.port()
    );
    let state = random();
    let nonce = random();
    let verifier = random();
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let cid = previous
        .as_ref()
        .map(|a| a.client_id.as_str())
        .unwrap_or("dynamic_agent_client");
    let mut url = reqwest::Url::parse(&format!("{ISSUER}/api/accounts/authorize")).unwrap();
    {
        let mut q = url.query_pairs_mut();
        q.extend_pairs(&[
            ("client_id", cid),
            ("ext_agent_host_id", &host),
            ("response_type", "code"),
            ("redirect_uri", &redirect),
            (
                "scope",
                "openid profile email offline_access resource.invoke chatgpt.tokens.use.direct",
            ),
            ("resource", RESOURCE),
            ("state", &state),
            ("nonce", &nonce),
            ("code_challenge_method", "S256"),
            ("code_challenge", &challenge),
        ]);
        if let Some(a) = &previous {
            if !a.id_token.is_empty() {
                q.append_pair("id_token_hint", &a.id_token);
            }
        } else {
            q.append_pair("agent_name_hint", "Lexi");
        }
    }
    open::that(url.as_str())
        .map_err(|_| err("Could not open the system browser for ChatGPT sign-in"))?;
    let (code, issued) = tokio::time::timeout(Duration::from_secs(300), async {
        loop {
            let (mut socket, _) = listener.accept().await?;
            let read = async {
                let mut request = Vec::new();
                let mut chunk = [0u8; 1024];
                while request.len() < 16384 {
                    let n = socket.read(&mut chunk).await?;
                    if n == 0 { break; }
                    request.extend_from_slice(&chunk[..n]);
                    if request.windows(4).any(|w| w == b"\r\n\r\n") { break; }
                }
                Ok::<_, std::io::Error>(request)
            };
            let request = match tokio::time::timeout(Duration::from_secs(3), read).await {
                Ok(Ok(bytes)) => bytes,
                _ => continue,
            };
            let request = String::from_utf8_lossy(&request);
            let target = request.lines().next().and_then(|l| l.strip_prefix("GET "))
                .and_then(|l| l.split_whitespace().next()).unwrap_or("");
            if !target.starts_with("/auth/callback?") { continue; }
            let result = callback(target, &state, previous.as_ref().map(|a| a.client_id.as_str()));
            let message = if result.is_ok() { "Return to Lexi to finish sign-in." }
                else { "Sign-in could not be verified. Return to Lexi." };
            let response = format!("HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nCache-Control: no-store\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{}", message.len(), message);
            let _ = socket.write_all(response.as_bytes()).await;
            return result;
        }
    }).await.map_err(|_| err("ChatGPT sign-in timed out; try again"))??;
    let tokens = token_request(&[
        ("grant_type", "authorization_code"),
        ("client_id", &issued),
        ("code", &code),
        ("code_verifier", &verifier),
        ("redirect_uri", &redirect),
        ("resource", RESOURCE),
    ])
    .await?;
    let id = tokens["id_token"]
        .as_str()
        .ok_or_else(|| err("Missing ChatGPT ID token"))?;
    let identity = validate_id(id, &issued, &nonce).await?;
    if previous.as_ref().is_some_and(|a| a.subject != identity.sub) {
        return Err(err(
            "ChatGPT account changed; add it under a different label",
        ));
    }
    let mut a = Account {
        client_id: issued,
        subject: identity.sub,
        email: identity.email,
        access_token: String::new(),
        refresh_token: String::new(),
        id_token: id.into(),
        scope: String::new(),
        expires_at: 0,
    };
    apply_tokens(&mut a, &tokens, true)?;
    {
        let _guard = lock().await?;
        let mut s = load()?;
        s.accounts.insert(account.into(), a);
        save(&s)?;
    }
    let provider = format!("chatgpt-{account}");
    let providers = config::load_providers()?;
    if providers["providers"].get(&provider).is_none() {
        config::add_provider(&provider,json!({"api":"responses","url":RESOURCE,"chatgpt_account":account,"catalog":{"path":"/models","fields":["id","display_name"]},"tag_rules":{"*": ["kind:chat","feature:tools"]}}).as_object().unwrap())?;
    }
    let models = crate::hosted::fetch_hosted_models(&provider).await?;
    for model in &models {
        if let Some(id) = model["id"].as_str() {
            config::upsert_model(
                &provider,
                id,
                json!({"name":model["display_name"]}).as_object().unwrap(),
                false,
            )?;
        }
    }
    if config::default_models(&provider)?.get("chat").is_none() {
        if let Some(model) = models.first().and_then(|m| m["id"].as_str()) {
            crate::models::set_default(&provider, model, Some("chat"))?;
        }
    }
    Ok(
        json!({"provider":provider,"account":account,"signed_in":true,"usage_url":USAGE,"message":"You're using your ChatGPT plan. Eligible requests share your plan limits. Choose a model from models hosted; manage limits and optional credits in ChatGPT settings."}),
    )
}
/// Revoke the renewable session and clear local tokens, retaining registration identity.
pub async fn chatgpt_logout(account: &str) -> Result<Value, HostError> {
    let _guard = lock().await?;
    let _action = account_action_lock()?;
    let mut s = load()?;
    let a = s
        .accounts
        .get_mut(account)
        .ok_or_else(|| err("Unknown ChatGPT account label"))?;
    let mut revoked = a.refresh_token.is_empty();
    if !revoked {
        let discovery: Result<Value, HostError> = async {
            Ok(client()?
                .get(format!("{ISSUER}/.well-known/openid-configuration"))
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?)
        }
        .await;
        if let Ok(d) = discovery {
            if let Some(endpoint) = d["revocation_endpoint"]
                .as_str()
                .filter(|u| u.starts_with("https://auth.openai.com/"))
            {
                for delay in [0, 1, 2] {
                    if delay > 0 {
                        tokio::time::sleep(Duration::from_secs(delay)).await;
                    }
                    if let Ok(r) = client()?
                        .post(endpoint)
                        .form(&[
                            ("token", a.refresh_token.as_str()),
                            ("token_type_hint", "refresh_token"),
                            ("client_id", &a.client_id),
                        ])
                        .send()
                        .await
                    {
                        if r.status() == 200 {
                            revoked = true;
                            break;
                        }
                        if !r.status().is_server_error() {
                            break;
                        }
                    }
                }
            }
        }
    }
    a.access_token.clear();
    a.refresh_token.clear();
    a.id_token.clear();
    a.expires_at = 0;
    save(&s)?;
    Ok(
        json!({"signed_in":false,"remote_revocation_confirmed":revoked,"message":if revoked{"Signed out"}else{"Signed out locally; remote revocation not confirmed. Disconnect Lexi in ChatGPT Settings."}}),
    )
}
pub(crate) fn enabled(cfg: &Value) -> bool {
    cfg.get("chatgpt_account").is_some()
}
pub(crate) async fn credential(cfg: &Value, endpoint: &str) -> Result<String, HostError> {
    if ![
        "https://api.openai.com/v1/responses",
        "https://api.openai.com/v1/models",
    ]
    .contains(&endpoint)
    {
        return Err(err(
            "ChatGPT plan credentials can only be sent to OpenAI's Responses or models endpoint",
        ));
    }
    let account = cfg["chatgpt_account"]
        .as_str()
        .ok_or_else(|| err("Missing ChatGPT account"))?;
    let _guard = lock().await?;
    let mut s = load()?;
    let a = s
        .accounts
        .get_mut(account)
        .ok_or_else(|| err("Sign in to ChatGPT first"))?;
    if a.access_token.is_empty() {
        return Err(err(
            "Sign in to ChatGPT again; API-key fallback is disabled",
        ));
    }
    if a.expires_at <= now() + 60 {
        let v = token_request(&[
            ("grant_type", "refresh_token"),
            ("client_id", &a.client_id),
            ("refresh_token", &a.refresh_token),
            ("resource", RESOURCE),
        ])
        .await?;
        apply_tokens(a, &v, false)?;
        save(&s)?;
    }
    Ok(s.accounts[account].access_token.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identity_rejects_bad_signature_issuer_audience_nonce_and_expiry() {
        use jsonwebtoken::{encode, Algorithm, DecodingKey, EncodingKey, Header};
        // Deliberately public test-only key; never used by Lexi authentication.
        let signing =
            EncodingKey::from_rsa_pem(include_bytes!("../tests/fixtures/test-only-rsa.pem"))
                .unwrap();
        let verification =
            DecodingKey::from_rsa_pem(include_bytes!("../tests/fixtures/test-only-rsa.pub.pem"))
                .unwrap();
        let valid =
            json!({"iss":ISSUER,"aud":"oaiapp_test","sub":"user","nonce":"nonce","exp":now()+3600});
        let token = encode(&Header::new(Algorithm::RS256), &valid, &signing).unwrap();
        assert!(verify_identity(&token, "oaiapp_test", "nonce", &verification).is_ok());
        for (claim, value) in [
            ("iss", json!("https://evil.example")),
            ("aud", json!("another-client")),
            ("nonce", json!("wrong")),
            ("exp", json!(now() - 3600)),
        ] {
            let mut invalid = valid.clone();
            invalid[claim] = value;
            let token = encode(&Header::new(Algorithm::RS256), &invalid, &signing).unwrap();
            assert!(verify_identity(&token, "oaiapp_test", "nonce", &verification).is_err());
        }
        let mut tampered = token.into_bytes();
        let last = tampered.len() - 8;
        tampered[last] = if tampered[last] == b'A' { b'B' } else { b'A' };
        assert!(verify_identity(
            std::str::from_utf8(&tampered).unwrap(),
            "oaiapp_test",
            "nonce",
            &verification
        )
        .is_err());
    }
    #[test]
    fn callback_binds_state_client_and_path() {
        assert_eq!(
            callback(
                "/auth/callback?state=s&code=c&client_id=oaiapp_123",
                "s",
                None
            )
            .unwrap(),
            ("c".into(), "oaiapp_123".into())
        );
        for url in [
            "/auth/callback?state=other&code=c&client_id=oaiapp_123",
            "/auth/callback?state=s&state=s&code=c&client_id=oaiapp_123",
            "/callback?state=s&code=c&client_id=oaiapp_123",
            "/auth/callback?state=s&code=c",
            "/auth/callback?state=s&error=access_denied&code=c&client_id=oaiapp_123",
        ] {
            assert!(callback(url, "s", None).is_err(), "{url}");
        }
        assert!(callback(
            "/auth/callback?state=s&code=c&client_id=oaiapp_other",
            "s",
            Some("oaiapp_original")
        )
        .is_err());
        assert!(callback(
            "/auth/callback?state=s&code=c",
            "s",
            Some("oaiapp_original")
        )
        .is_ok());
    }
    #[test]
    fn scopes_are_required_and_refresh_preserves_grant() {
        let mut a = Account {
            client_id: "oaiapp_test".into(),
            subject: "sub".into(),
            email: "".into(),
            access_token: "".into(),
            refresh_token: "".into(),
            id_token: "".into(),
            scope: "".into(),
            expires_at: 0,
        };
        let mut v = json!({"access_token":"access","refresh_token":"refresh","token_type":"Bearer","expires_in":3600,"scope":"openid"});
        assert!(apply_tokens(&mut a, &v, true).is_err());
        v["scope"] = json!(PLAN_SCOPE);
        apply_tokens(&mut a, &v, true).unwrap();
        let refresh = json!({"access_token":"new","refresh_token":"rotated","token_type":"Bearer","expires_in":3600});
        apply_tokens(&mut a, &refresh, false).unwrap();
        assert_eq!(a.refresh_token, "rotated");
        assert_eq!(a.scope, PLAN_SCOPE);
        assert!(a.expires_at > now());
    }
    #[tokio::test]
    async fn rejects_foreign_endpoints_before_loading_secrets() {
        for url in [
            "http://api.openai.com/v1/responses",
            "https://evil.example/v1/responses",
            "https://api.openai.com/v1/embeddings",
            "https://api.openai.com/v1/responses?redirect=evil",
        ] {
            assert!(credential(&json!({"chatgpt_account":"personal"}), url)
                .await
                .is_err());
        }
    }
    #[test]
    fn labels_cannot_escape_storage_or_provider_names() {
        for value in ["", "../personal", "foo/bar", "with space", "💥"] {
            assert!(label(value).is_err());
        }
        assert!(label("work-2").is_ok());
    }
}
