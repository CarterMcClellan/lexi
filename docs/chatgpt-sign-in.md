# Sign in with ChatGPT

Lexi can use an eligible ChatGPT plan for Responses requests, through OpenAI's
[documented open-source sign-in flow](https://developers.openai.com/siwc/token-sharing-open-source).
This integration is for locally run open-source installations. Hosted or paid
services require OpenAI approval.

In the desktop app, open **Providers → Use your ChatGPT plan → Continue with
ChatGPT**. Complete consent in the system browser. Lexi creates a separate
`chatgpt-personal` provider and discovers your account's available models. Select
that provider in Chat. The original `openai` provider continues to use API billing.
There is no automatic fallback between those billing methods.

CLI:

```sh
lexi providers login personal
lexi providers accounts
lexi models hosted chatgpt-personal
lexi respond chatgpt-personal "Hello"
lexi providers logout personal
```

Use another label, such as `work`, to add another account or workspace. Reuse a
label when signing in again to the same registration. Each account has its own
provider/model selection. Python exposes the same operations as
`lexi.providers.login`, `.accounts`, and `.logout`.

Eligible requests share your ChatGPT plan limits; optional credits depend on your
ChatGPT settings. [Manage usage](https://chatgpt.com/settings/usage). Only Responses
inference is supported: requests always stream with `store: false`, including
when Lexi displays the final answer all at once. Background jobs, embeddings, and
the separate Images API are not enabled by this login. Provider errors never
trigger paid API-key fallback. Live sign-in and inference require your consent
and an eligible account; offline tests do not establish account eligibility.

Credentials live in `$LEXI_HOME/chatgpt/accounts.json` (by default
`~/.lexi-cli/chatgpt/`), with owner-only Unix permissions and atomic writes.
A process-shared lock serializes token refresh. OAuth uses a loopback callback,
state, nonce, PKCE, and signed ID-token validation. Tokens stay in the native
runtime and are not returned to the webview, CLI JSON, or Python. Sign-out attempts
remote revocation, clears tokens, and retains the registration and host identity.
If revocation cannot be confirmed, disconnect Lexi from ChatGPT settings.

Anthropic subscription authentication is not part of this change. Its current
[third-party guidance](https://support.claude.com/en/articles/13189465-log-in-to-your-claude-account)
does not provide the same guaranteed use of subscription allowances.
