const http = require('http');
const { spawn } = require('child_process');
const fs = require('fs');
const path = require('path');
let YAML;
try {
  YAML = require('yaml');
} catch (err) {
  YAML = null; // will surface a helpful error later
}

const PORT = process.env.PORT || 3000;
const ROOT_DIR = path.resolve(__dirname, '..', '..'); // lexi-gui root
const WORKSPACE_ROOT = path.resolve(__dirname, '..', '..', '..'); // repo root containing cli
const PUBLIC_DIR = path.join(ROOT_DIR, 'public');
const INDEX_PATH = path.join(PUBLIC_DIR, 'index.html');
const USER_PROVIDERS_CONFIG = path.join(process.env.HOME || '', '.lexi-cli', 'providers', 'providers-config.yaml');
const USER_CONFIG = path.join(process.env.HOME || '', '.lexi-cli', 'config.json');
const REPO_PROVIDERS_CONFIG = path.join(WORKSPACE_ROOT, 'providers', 'providers-config.yaml');

const localLexiBin = path.join(WORKSPACE_ROOT, 'lexi-cli', 'cli');
const LEXI_BIN = process.env.LEXI_BIN || (fs.existsSync(localLexiBin) ? localLexiBin : 'lexi');
const LEXI_PROMPT = process.env.LEXI_PROMPT || 'lexi>';

function sendJson(res, statusCode, payload) {
  res.writeHead(statusCode, {
    'Content-Type': 'application/json',
    'Access-Control-Allow-Origin': '*',
    'X-Lexi-Prompt': LEXI_PROMPT,
  });
  res.end(JSON.stringify(payload));
}

function runLexi(args = []) {
  return new Promise((resolve) => {
    const proc = spawn(LEXI_BIN, args, {
      cwd: WORKSPACE_ROOT,
      env: { ...process.env, HOME: process.env.HOME },
    });
    let stdout = '';
    let stderr = '';
    proc.stdout.on('data', (chunk) => {
      stdout += chunk.toString();
    });
    proc.stderr.on('data', (chunk) => {
      stderr += chunk.toString();
    });
    proc.on('close', (code) => {
      if (code !== 0) {
        console.error(`[lexi-gui] lexi failed (${code}):`, LEXI_BIN, args.join(' '), stderr.trim());
      }
      resolve({ code, stdout, stderr, args });
    });
    proc.on('error', (err) => {
      console.error(`[lexi-gui] failed to spawn lexi:`, err);
      resolve({ code: 1, stdout, stderr: err.message });
    });
  });
}

function serveIndex(res) {
  const stream = fs.createReadStream(INDEX_PATH);
  let headersSent = false;
  stream.on('open', () => {
    if (!headersSent) {
      res.writeHead(200, { 'Content-Type': 'text/html' });
      headersSent = true;
    }
    stream.pipe(res);
  });
  stream.on('error', () => {
    if (!headersSent) {
      res.writeHead(500, { 'Content-Type': 'text/plain' });
      headersSent = true;
      res.end('Failed to load UI file.');
    } else {
      res.destroy();
    }
  });
}

function parsePromptList(rawOutput) {
  const text = rawOutput.trim();
  if (!text) return [];

  const attemptArrayParse = (t) => {
    try {
      const arr = JSON.parse(t);
      return Array.isArray(arr) ? arr : null;
    } catch {
      return null;
    }
  };

  // Try to coerce concatenated objects into an array
  const arrayText = `[${text.replace(/}\s*{\s*/g, '},{')}]`;
  const arr = attemptArrayParse(arrayText);
  const parts = arr && arr.length ? arr : text.split(/\n{2,}/);

  const prompts = [];
  for (const part of parts) {
    const chunk = typeof part === 'string' ? part : JSON.stringify(part);
    if (!chunk.trim()) continue;
    try {
      const obj = typeof part === 'string' ? JSON.parse(chunk) : part;
      const [name, payload] = Object.entries(obj)[0] || [];
      if (name && payload && typeof payload === 'object') {
        prompts.push({ name, ...payload });
      }
    } catch {
      continue;
    }
  }
  return prompts;
}

// --- Prompt handlers ---

async function handleListPrompts(res, logFn) {
  const { code, stdout, stderr, args } = await runLexi(['prompts', 'list', '-r']);
  logFn && logFn(args);
  if (code !== 0) {
    sendJson(res, 500, { error: 'Failed to list prompts', details: stderr.trim() });
    return;
  }
  const prompts = parsePromptList(stdout);
  const normalized = prompts.map((p) => ({
    name: p.name,
    prompt: p.prompt || '',
    max_tokens: p.max_tokens,
    temperature: p.temperature,
    role: p.role,
    isActive: p.name === '$$',
  }));
  sendJson(res, 200, { prompts: normalized });
}

async function handleSavePrompt(req, res, body, logFn) {
  const { name, prompt, max_tokens, temperature, role } = body || {};
  if (!prompt || typeof prompt !== 'string') {
    sendJson(res, 400, { error: 'Prompt text is required' });
    return;
  }

  const args = ['prompts', 'set'];
  if (name) {
    args.push(name);
  }
  args.push('--prompt', prompt);
  if (max_tokens != null) {
    args.push('--max_tokens', String(max_tokens));
  }
  if (temperature != null) {
    args.push('--temperature', String(temperature));
  }
  if (role) {
    args.push('--role', role);
  }

  const { code, stderr, args: usedArgs } = await runLexi(args);
  logFn && logFn(usedArgs);
  if (code !== 0) {
    sendJson(res, 500, { error: 'Failed to save prompt', details: stderr.trim() });
    return;
  }

  await handleListPrompts(res, logFn);
}

async function handleDeletePrompt(res, name, logFn) {
  if (!name) {
    sendJson(res, 400, { error: 'Prompt name is required' });
    return;
  }
  const { code, stderr, args } = await runLexi(['prompts', 'rm', name]);
  logFn && logFn(args);
  if (code !== 0) {
    sendJson(res, 500, { error: 'Failed to delete prompt', details: stderr.trim() });
    return;
  }
  await handleListPrompts(res, logFn);
}

async function handleSetActive(res, name, logFn) {
  if (!name) {
    sendJson(res, 400, { error: 'Prompt name is required' });
    return;
  }

  // Fetch the prompt in raw form
  const listArgs = ['prompts', 'list', name, '-r'];
  const { code: listCode, stdout, stderr: listErr, args: usedListArgs } = await runLexi(listArgs);
  logFn && logFn(usedListArgs);
  if (listCode !== 0) {
    sendJson(res, 500, { error: 'Failed to load prompt', details: listErr.trim() });
    return;
  }
  const prompts = parsePromptList(stdout);
  const payload = prompts.find((p) => p.name === name);
  if (!payload) {
    sendJson(res, 404, { error: `Prompt ${name} not found` });
    return;
  }

  const args = ['prompts', 'set', '--prompt', payload.prompt || ''];
  if (payload.max_tokens != null) args.push('--max_tokens', String(payload.max_tokens));
  if (payload.temperature != null) args.push('--temperature', String(payload.temperature));
  if (payload.role) args.push('--role', payload.role);

  const { code, stderr, args: usedSetArgs } = await runLexi(args); // no name -> active $$
  logFn && logFn(usedSetArgs);
  if (code !== 0) {
    sendJson(res, 500, { error: 'Failed to set active prompt', details: stderr.trim() });
    return;
  }

  await handleListPrompts(res, logFn);
}

function handlePrompt(req, res, logFn) {
  let body = '';
  req.on('data', (chunk) => {
    body += chunk.toString();
    if (body.length > 1e6) req.destroy();
  });

  req.on('end', () => {
    let parsed = {};
    try {
      parsed = JSON.parse(body || '{}');
    } catch (err) {
      sendJson(res, 400, { error: 'Invalid JSON body' });
      return;
    }

    if (req.method === 'GET') {
      handleListPrompts(res, logFn);
      return;
    }

    if (req.method === 'POST' && req.url === '/api/prompts') {
      handleSavePrompt(req, res, parsed, logFn);
      return;
    }

    if (req.method === 'POST' && req.url === '/api/prompts/active') {
      handleSetActive(res, parsed.name, logFn);
      return;
    }

    if (req.method === 'POST' && req.url === '/api/prompts/delete') {
      handleDeletePrompt(res, parsed.name, logFn);
      return;
    }

    sendJson(res, 404, { error: 'Not Found' });
  });
}

// --- Provider handlers ---

function loadProvidersConfig() {
  const pathCandidates = [USER_PROVIDERS_CONFIG, REPO_PROVIDERS_CONFIG];
  const found = pathCandidates.find((p) => p && fs.existsSync(p));
  if (!found) {
    return { error: 'providers-config.yaml not found. Run lexi-cli to seed defaults or create ~/.lexi-cli/providers/providers-config.yaml.' };
  }
  if (!YAML) {
    return { error: "Missing dependency 'yaml'. Install with: npm install yaml" };
  }
  try {
    const text = fs.readFileSync(found, 'utf8');
    const data = YAML.parse(text) || {};
    return { config: data, source: found };
  } catch (err) {
    return { error: `Failed to read providers-config.yaml: ${err.message}` };
  }
}

function providerHasApiKeyFile(name) {
  const keyPath = path.join(process.env.HOME || '', '.lexi-cli', 'providers', name, 'api-key');
  try {
    const content = fs.readFileSync(keyPath, 'utf8').trim();
    return content.length > 0;
  } catch {
    return false;
  }
}

function handleProviders(res) {
  const { config, error, source } = loadProvidersConfig();
  if (error) {
    sendJson(res, 500, { error });
    return;
  }
  const providers = (config && config.providers && typeof config.providers === 'object') ? config.providers : {};
  const normalized = Object.entries(providers).map(([name, meta]) => {
    const hasKeyFile = providerHasApiKeyFile(name);
    const hasKey = hasKeyFile || !!(meta?.api_key);
    const hasUrl = !!(meta?.url);
    const hasModel = !!(meta?.default_model);
    return {
      name,
      url: meta?.url || '',
      hosted_models: meta?.hosted_models || '',
      hosted_model_fields: Array.isArray(meta?.hosted_model_fields) ? meta.hosted_model_fields : [],
      default_model: meta?.default_model || '',
      api: meta?.api || '',
      api_key: hasKey ? '[set]' : '[missing]',
      configured: hasUrl && hasKey && hasModel,
      source,
    };
  });
  sendJson(res, 200, { providers: normalized, source });
}

async function handleApiKey(req, res, body, logFn) {
  const { action, provider, value, reveal } = body || {};
  if (!provider || typeof provider !== 'string') {
    sendJson(res, 400, { error: 'provider is required' });
    return;
  }
  if (!action || !['set', 'show', 'remove'].includes(action)) {
    sendJson(res, 400, { error: 'action must be set, show, or remove' });
    return;
  }

  let args;
  if (action === 'set') {
    if (!value || typeof value !== 'string' || !value.trim()) {
      sendJson(res, 400, { error: 'value is required for set action' });
      return;
    }
    args = ['providers', 'api-key', 'set', provider, '--value', value.trim()];
  } else if (action === 'show') {
    args = ['providers', 'api-key', 'show', provider];
    if (reveal === true) args.push('--reveal');
  } else {
    args = ['providers', 'api-key', 'remove', provider];
  }

  const result = await runLexi(args);
  logFn && logFn(result.args);
  if (result.code !== 0) {
    sendJson(res, 500, { error: result.stderr.trim() || 'Command failed' });
    return;
  }
  sendJson(res, 200, { output: result.stdout.trim() });
}

// --- Models handlers ---

async function handleModelsList(res, provider, logFn) {
  if (!provider) {
    sendJson(res, 400, { error: 'provider query param is required' });
    return;
  }
  const { code, stdout, stderr, args } = await runLexi(['models', 'list', provider, '-r']);
  logFn && logFn(args);
  if (code !== 0) {
    sendJson(res, 500, { error: `Failed to list models for ${provider}`, details: stderr.trim() });
    return;
  }
  const stripAnsi = (txt = '') => txt.replace(/\x1B\[[0-9;]*[A-Za-z]/g, '');
  const text = stripAnsi(stdout).trim();
  let models = [];
  if (text) {
    let parsed;
    try {
      parsed = JSON.parse(text);
    } catch {
      try {
        parsed = YAML && YAML.parse(text);
      } catch {
        parsed = null;
      }
    }
    if (parsed) {
      if (Array.isArray(parsed)) {
        models = parsed;
      } else if (parsed && typeof parsed === 'object') {
        const src = parsed.models || parsed;
        if (typeof src === 'object' && !Array.isArray(src)) {
          // Dict keyed by model ID — convert to array with id injected
          models = Object.entries(src).map(([id, meta]) => ({
            id,
            ...(meta && typeof meta === 'object' ? meta : {}),
          }));
        } else if (Array.isArray(src)) {
          models = src;
        }
      }
    } else {
      // fallback: treat as line-separated
      models = text.split('\n').filter(Boolean).map((l) => ({ id: l.trim() }));
    }
  }
  sendJson(res, 200, { provider, models });
}

async function handleModelsDefault(res, provider, logFn) {
  if (!provider) {
    sendJson(res, 400, { error: 'provider query param is required' });
    return;
  }
  const { code, stdout, stderr, args } = await runLexi(['models', 'default', provider]);
  logFn && logFn(args);
  if (code !== 0) {
    sendJson(res, 500, { error: `Failed to get default model for ${provider}`, details: stderr.trim() });
    return;
  }
  const output = stdout.trim();
  const match = output.match(/Default model for .+?:\s*(.+)/);
  sendJson(res, 200, { provider, default_model: match ? match[1].trim() : null });
}

async function handleSetModelsDefault(res, body, logFn) {
  const { provider, model } = body || {};
  if (!provider || !model) {
    sendJson(res, 400, { error: 'provider and model are required' });
    return;
  }
  const { code, stdout, stderr, args } = await runLexi(['models', 'default', provider, model]);
  logFn && logFn(args);
  if (code !== 0) {
    sendJson(res, 500, { error: `Failed to set default model`, details: (stderr || stdout).trim() });
    return;
  }
  sendJson(res, 200, { provider, default_model: model, output: stdout.trim() });
}

async function handleModelsAliases(res, provider, logFn) {
  if (!provider) {
    sendJson(res, 400, { error: 'provider query param is required' });
    return;
  }
  const { code, stdout, stderr, args } = await runLexi(['models', 'alias', 'list', provider, '-r']);
  logFn && logFn(args);
  if (code !== 0) {
    sendJson(res, 500, { error: `Failed to load aliases for ${provider}`, details: stderr.trim() });
    return;
  }
  const stripAnsi = (txt = '') => txt.replace(/\x1B\[[0-9;]*[A-Za-z]/g, '');
  const text = stripAnsi(stdout).trim();
  let aliases = {};
  if (text) {
    try {
      if (YAML) {
        const parsed = YAML.parse(text);
        if (parsed && typeof parsed === 'object') {
          aliases = parsed.aliases && typeof parsed.aliases === 'object' ? parsed.aliases : parsed;
        }
      } else {
        aliases = JSON.parse(text);
      }
    } catch {
      // Parse individual "alias -> model" lines as fallback
      for (const line of text.split('\n')) {
        const m = line.match(/^\s*(\S+)\s*->\s*(\S+)/);
        if (m) aliases[m[1]] = m[2];
      }
    }
  }
  sendJson(res, 200, { provider, aliases });
}

async function handleSetModelAlias(res, body, logFn) {
  const { provider, alias, model } = body || {};
  if (!provider || !alias || !model) {
    sendJson(res, 400, { error: 'provider, alias, and model are required' });
    return;
  }
  const { code, stdout, stderr, args } = await runLexi(['models', 'alias', 'set', provider, alias, model]);
  logFn && logFn(args);
  if (code !== 0) {
    sendJson(res, 500, { error: 'Failed to set alias', details: (stderr || stdout).trim() });
    return;
  }
  sendJson(res, 200, { provider, alias, model, output: stdout.trim() });
}

async function handleDeleteModelAlias(res, body, logFn) {
  const { provider, alias } = body || {};
  if (!provider || !alias) {
    sendJson(res, 400, { error: 'provider and alias are required' });
    return;
  }
  const { code, stdout, stderr, args } = await runLexi(['models', 'alias', 'rm', provider, alias]);
  logFn && logFn(args);
  if (code !== 0) {
    sendJson(res, 500, { error: 'Failed to remove alias', details: (stderr || stdout).trim() });
    return;
  }
  sendJson(res, 200, { provider, alias, output: stdout.trim() });
}

async function handleModelsHosted(res, provider, logFn) {
  if (!provider) {
    sendJson(res, 400, { error: 'provider query param is required' });
    return;
  }
  const { code, stdout, stderr, args } = await runLexi(['models', 'hosted', provider, '-r']);
  logFn && logFn(args);
  if (code !== 0) {
    sendJson(res, 500, { error: `Failed to fetch hosted models for ${provider}`, details: stderr.trim() });
    return;
  }
  const stripAnsi = (txt = '') => txt.replace(/\x1B\[[0-9;]*[A-Za-z]/g, '');
  const text = stripAnsi(stdout).trim();
  let models = [];
  if (text) {
    try {
      const parsed = JSON.parse(text);
      if (Array.isArray(parsed)) {
        models = parsed;
      } else if (parsed && typeof parsed === 'object') {
        const arr = parsed.data || parsed.models || Object.values(parsed);
        models = Array.isArray(arr) ? arr : [];
      }
    } catch {
      models = text.split('\n').filter(Boolean).map((l) => ({ id: l.trim() }));
    }
  }
  sendJson(res, 200, { provider, models });
}

async function handleAddModel(res, body, logFn) {
  const { provider, model, name, description, max_tokens, context_window, set_default } = body || {};
  if (!provider || !model) {
    sendJson(res, 400, { error: 'provider and model are required' });
    return;
  }
  const args = ['models', 'add', provider, model];
  if (name) args.push('--name', name);
  if (description) args.push('--description', description);
  if (max_tokens != null) args.push('--max_tokens', String(max_tokens));
  if (context_window != null) args.push('--context_window', String(context_window));
  if (set_default) args.push('--default');
  const { code, stdout, stderr, args: usedArgs } = await runLexi(args);
  logFn && logFn(usedArgs);
  if (code !== 0) {
    sendJson(res, 500, { error: 'Failed to add model', details: (stderr || stdout).trim() });
    return;
  }
  sendJson(res, 200, { provider, model, output: stdout.trim() });
}

async function handleSetModel(res, body, logFn) {
  const { provider, model, name, description, max_tokens, context_window, set_default } = body || {};
  if (!provider || !model) {
    sendJson(res, 400, { error: 'provider and model are required' });
    return;
  }
  const args = ['models', 'set', provider, model];
  if (name) args.push('--name', name);
  if (description) args.push('--description', description);
  if (max_tokens != null) args.push('--max_tokens', String(max_tokens));
  if (context_window != null) args.push('--context_window', String(context_window));
  if (set_default) args.push('--default');
  const { code, stdout, stderr, args: usedArgs } = await runLexi(args);
  logFn && logFn(usedArgs);
  if (code !== 0) {
    sendJson(res, 500, { error: 'Failed to update model', details: (stderr || stdout).trim() });
    return;
  }
  sendJson(res, 200, { provider, model, output: stdout.trim() });
}

async function handleDeleteModel(res, body, logFn) {
  const { provider, model } = body || {};
  if (!provider || !model) {
    sendJson(res, 400, { error: 'provider and model are required' });
    return;
  }
  const { code, stdout, stderr, args } = await runLexi(['models', 'rm', provider, model]);
  logFn && logFn(args);
  if (code !== 0) {
    sendJson(res, 500, { error: 'Failed to remove model', details: (stderr || stdout).trim() });
    return;
  }
  sendJson(res, 200, { provider, model, output: stdout.trim() });
}

// --- Config handlers ---

function handleGetConfig(res) {
  const defaults = { cli_name: 'lexi', prompt: 'lexi', prompt_delimiter: '>', edit_mode: 'emacs' };
  try {
    const raw = fs.readFileSync(USER_CONFIG, 'utf8');
    const cfg = JSON.parse(raw);
    sendJson(res, 200, { ...defaults, ...cfg });
  } catch {
    sendJson(res, 200, defaults);
  }
}

async function handleSetConfig(req, res, body, logFn) {
  const cliKeys = ['cli_name', 'prompt', 'prompt_delimiter', 'edit_mode'];
  const guiKeys = ['theme'];

  // Set CLI keys via lexi config set
  for (const key of cliKeys) {
    if (body[key] !== undefined) {
      const { args } = await runLexi(['config', 'set', key, String(body[key])]);
      logFn && logFn(args);
    }
  }

  // Write GUI-only keys directly to config.json
  const hasGuiKeys = guiKeys.some((k) => body[k] !== undefined);
  if (hasGuiKeys) {
    let existing = {};
    try {
      existing = JSON.parse(fs.readFileSync(USER_CONFIG, 'utf8'));
    } catch {
      // file may not exist yet
    }
    for (const key of guiKeys) {
      if (body[key] !== undefined) {
        existing[key] = body[key];
      }
    }
    const dir = path.dirname(USER_CONFIG);
    if (!fs.existsSync(dir)) {
      fs.mkdirSync(dir, { recursive: true });
    }
    fs.writeFileSync(USER_CONFIG, JSON.stringify(existing, null, 2));
  }

  handleGetConfig(res);
}

// --- API definition handlers ---

async function handleApiList(res, logFn) {
  const { code, stdout, stderr, args } = await runLexi(['api', 'list', '-r']);
  logFn && logFn(args);
  if (code !== 0) {
    sendJson(res, 500, { error: 'Failed to list API definitions', details: stderr.trim() });
    return;
  }
  const stripAnsi = (txt = '') => txt.replace(/\x1B\[[0-9;]*[A-Za-z]/g, '');
  const text = stripAnsi(stdout).trim();
  let apis = [];
  if (text) {
    let parsed;
    try {
      parsed = JSON.parse(text);
    } catch {
      try {
        parsed = YAML && YAML.parse(text);
      } catch {
        parsed = null;
      }
    }
    if (parsed) {
      if (Array.isArray(parsed)) {
        apis = parsed;
      } else if (typeof parsed === 'object') {
        // Dict keyed by name — convert to array
        apis = Object.entries(parsed).map(([name, def]) => ({
          name,
          ...(def && typeof def === 'object' ? def : {}),
        }));
      }
    }
  }
  sendJson(res, 200, { apis });
}

async function handleApiShow(res, name, logFn) {
  if (!name) {
    sendJson(res, 400, { error: 'API definition name is required' });
    return;
  }
  const { code, stdout, stderr, args } = await runLexi(['api', 'show', name, '-r']);
  logFn && logFn(args);
  if (code !== 0) {
    sendJson(res, 500, { error: `Failed to load API definition ${name}`, details: stderr.trim() });
    return;
  }
  const stripAnsi = (txt = '') => txt.replace(/\x1B\[[0-9;]*[A-Za-z]/g, '');
  const text = stripAnsi(stdout).trim();
  let definition = null;
  if (text) {
    try {
      definition = JSON.parse(text);
    } catch {
      try {
        definition = YAML && YAML.parse(text);
      } catch {
        definition = null;
      }
    }
  }
  if (!definition) {
    sendJson(res, 404, { error: `API definition ${name} not found` });
    return;
  }
  sendJson(res, 200, { name, definition });
}

async function handleApiCreate(res, body, logFn) {
  const { name, from: fromName, path: reqPath, text_path } = body || {};
  if (!name || typeof name !== 'string' || !name.trim()) {
    sendJson(res, 400, { error: 'name is required' });
    return;
  }
  const args = ['api', 'create', name.trim()];
  if (fromName) args.push('--from', fromName);
  if (reqPath) args.push('--path', reqPath);
  if (text_path) args.push('--text-path', text_path);

  const { code, stdout, stderr, args: usedArgs } = await runLexi(args);
  logFn && logFn(usedArgs);
  if (code !== 0) {
    sendJson(res, 500, { error: 'Failed to create API definition', details: (stderr || stdout).trim() });
    return;
  }
  sendJson(res, 200, { name: name.trim(), output: stdout.trim() });
}

async function handleApiEdit(res, body, logFn) {
  const { name, path: reqPath, text_path, params, metadata } = body || {};
  if (!name || typeof name !== 'string' || !name.trim()) {
    sendJson(res, 400, { error: 'name is required' });
    return;
  }
  const args = ['api', 'edit', name.trim()];
  if (reqPath) args.push('--path', reqPath);
  if (text_path) args.push('--text-path', text_path);
  if (Array.isArray(params)) {
    for (const p of params) {
      if (typeof p === 'string' && p.trim()) {
        args.push('--set-param', p.trim());
      }
    }
  }
  if (Array.isArray(metadata)) {
    for (const m of metadata) {
      if (typeof m === 'string' && m.trim()) {
        args.push('--set-meta', m.trim());
      }
    }
  }

  const { code, stdout, stderr, args: usedArgs } = await runLexi(args);
  logFn && logFn(usedArgs);
  if (code !== 0) {
    sendJson(res, 500, { error: 'Failed to edit API definition', details: (stderr || stdout).trim() });
    return;
  }
  sendJson(res, 200, { name: name.trim(), output: stdout.trim() });
}

async function handleApiDelete(res, body, logFn) {
  const { name } = body || {};
  if (!name || typeof name !== 'string' || !name.trim()) {
    sendJson(res, 400, { error: 'name is required' });
    return;
  }
  const { code, stdout, stderr, args } = await runLexi(['api', 'rm', name.trim()]);
  logFn && logFn(args);
  if (code !== 0) {
    sendJson(res, 500, { error: 'Failed to delete API definition', details: (stderr || stdout).trim() });
    return;
  }
  sendJson(res, 200, { name: name.trim(), output: stdout.trim() });
}

async function handleApiPreview(res, body, logFn) {
  const { yaml: yamlContent, name, prompt: samplePrompt, model: sampleModel, role: sampleRole } = body || {};
  let usingFile = false;
  let tempPath = null;
  const args = ['api', 'preview'];
  if (samplePrompt) args.push('--prompt', String(samplePrompt));
  if (sampleModel) args.push('--model', String(sampleModel));
  if (sampleRole) args.push('--role', String(sampleRole));

  try {
    if (yamlContent && typeof yamlContent === 'string') {
      const os = require('os');
      const tmpdir = os.tmpdir();
      tempPath = path.join(tmpdir, `lexi-api-preview-${process.pid}-${Date.now()}.yaml`);
      fs.writeFileSync(tempPath, yamlContent, 'utf8');
      args.push('--file', tempPath);
      usingFile = true;
    } else if (name && typeof name === 'string') {
      args.push(name);
    } else {
      sendJson(res, 400, { error: 'Provide either yaml or name' });
      return;
    }

    const { code, stdout, stderr } = await runLexi(args);
    logFn && logFn(args);
    if (code !== 0) {
      const stripAnsi = (txt = '') => txt.replace(/\x1B\[[0-9;]*[A-Za-z]/g, '');
      sendJson(res, 400, { error: stripAnsi((stderr || stdout).trim()) || 'preview failed' });
      return;
    }
    let parsed;
    try {
      parsed = JSON.parse(stdout);
    } catch (err) {
      sendJson(res, 500, { error: 'Could not parse preview output', details: err.message });
      return;
    }
    sendJson(res, 200, parsed);
  } finally {
    if (usingFile && tempPath) {
      try { fs.unlinkSync(tempPath); } catch (_e) { /* best-effort cleanup */ }
    }
  }
}

async function handleApiValidate(res, name, logFn) {
  if (!name) {
    sendJson(res, 400, { error: 'API definition name is required' });
    return;
  }
  const { code, stdout, stderr, args } = await runLexi(['api', 'validate', name]);
  logFn && logFn(args);
  const stripAnsi = (txt = '') => txt.replace(/\x1B\[[0-9;]*[A-Za-z]/g, '');
  const output = stripAnsi((stdout || stderr).trim());
  sendJson(res, 200, { name, valid: code === 0, output });
}

async function handleApiRaw(res, name, logFn) {
  if (!name) {
    sendJson(res, 400, { error: 'API definition name is required' });
    return;
  }
  const apiDir = path.join(process.env.HOME || '', '.lexi-cli', 'apis');
  const filePath = path.join(apiDir, `${name}.yaml`);
  try {
    const content = fs.readFileSync(filePath, 'utf8');
    sendJson(res, 200, { name, yaml: content });
  } catch {
    sendJson(res, 404, { error: `API definition file not found: ${name}.yaml` });
  }
}

async function handleApiSaveRaw(res, body, logFn) {
  const { name, yaml: yamlContent } = body || {};
  if (!name || typeof name !== 'string' || !name.trim()) {
    sendJson(res, 400, { error: 'name is required' });
    return;
  }
  if (!yamlContent || typeof yamlContent !== 'string') {
    sendJson(res, 400, { error: 'yaml content is required' });
    return;
  }
  // Validate the YAML is parseable
  if (!YAML) {
    sendJson(res, 500, { error: "Missing dependency 'yaml'. Install with: npm install yaml" });
    return;
  }
  try {
    YAML.parse(yamlContent);
  } catch (err) {
    sendJson(res, 400, { error: `Invalid YAML: ${err.message}` });
    return;
  }
  const apiDir = path.join(process.env.HOME || '', '.lexi-cli', 'apis');
  const filePath = path.join(apiDir, `${name.trim()}.yaml`);
  try {
    if (!fs.existsSync(apiDir)) {
      fs.mkdirSync(apiDir, { recursive: true });
    }
    fs.writeFileSync(filePath, yamlContent);
  } catch (err) {
    sendJson(res, 500, { error: `Failed to write API definition: ${err.message}` });
    return;
  }
  // Validate after saving
  const { code, stdout, stderr } = await runLexi(['api', 'validate', name.trim()]);
  logFn && logFn(['api', 'validate', name.trim()]);
  const stripAnsi = (txt = '') => txt.replace(/\x1B\[[0-9;]*[A-Za-z]/g, '');
  sendJson(res, 200, {
    name: name.trim(),
    valid: code === 0,
    validation: stripAnsi((stdout || stderr).trim()),
  });
}

// --- Respond handler ---

// --- Tool preset / options lookup ---------------------------------------

function loadApiDefinitionFromDisk(apiName) {
  if (!apiName || !YAML) return null;
  const candidates = [
    path.join(process.env.HOME || '', '.lexi-cli', 'apis', `${apiName}.yaml`),
    path.join(WORKSPACE_ROOT, 'providers', 'apis', `${apiName}.yaml`),
  ];
  for (const p of candidates) {
    if (fs.existsSync(p)) {
      try {
        return YAML.parse(fs.readFileSync(p, 'utf8'));
      } catch {
        return null;
      }
    }
  }
  return null;
}

function presetsForProvider(providerName) {
  const { config } = loadProvidersConfig() || {};
  const providers = (config && config.providers) || {};
  const meta = providers[providerName];
  if (!meta || !meta.api) return [];
  const def = loadApiDefinitionFromDisk(meta.api);
  if (!def) return [];
  const inline = (meta.tool_presets && typeof meta.tool_presets === 'object') ? meta.tool_presets : {};
  const fromApi = (def.tool_presets && typeof def.tool_presets === 'object') ? def.tool_presets : {};
  return [...new Set([...Object.keys(fromApi), ...Object.keys(inline)])].sort();
}

function handleRespondOptions(res, providersParam) {
  const providers = Array.isArray(providersParam)
    ? providersParam
    : (typeof providersParam === 'string' ? providersParam.split(',').map((s) => s.trim()).filter(Boolean) : []);
  const out = {};
  for (const name of providers) {
    out[name] = presetsForProvider(name);
  }
  // Union with provenance (provider tags) makes the UI checkbox list intuitive.
  const all = {};
  for (const [provName, presets] of Object.entries(out)) {
    for (const p of presets) {
      if (!all[p]) all[p] = [];
      all[p].push(provName);
    }
  }
  const presets = Object.keys(all).sort().map((name) => ({ name, providers: all[name] }));
  sendJson(res, 200, { per_provider: out, presets });
}

// --- Respond (blocked / buffered) ---------------------------------------

function buildRespondArgs(provider, opts) {
  const { model, tools, cite, verbose, stream } = opts || {};
  const args = ['respond', provider, '$$'];
  if (model) args.push('--model', model);
  if (Array.isArray(tools) && tools.length) {
    // Filter to presets this provider actually supports — silently drop the rest
    // so the user can pick a union across providers without per-provider errors.
    const supported = new Set(presetsForProvider(provider));
    const matched = tools.filter((t) => supported.has(t));
    if (matched.length) args.push('--tool', matched.join(','));
  }
  if (cite) args.push('--cite');
  if (verbose) args.push('--verbose');
  if (stream) args.push('--stream');
  return args;
}

async function handleRespond(req, res, body, logFn) {
  const { prompt, providers, models, tools, cite, verbose } = body || {};
  if (!prompt || typeof prompt !== 'string' || !prompt.trim()) {
    sendJson(res, 400, { error: 'prompt is required' });
    return;
  }
  if (!Array.isArray(providers) || !providers.length) {
    sendJson(res, 400, { error: 'at least one provider must be selected' });
    return;
  }
  const modelMap = models && typeof models === 'object' ? models : {};

  const setArgs = ['prompts', 'set', '--prompt', prompt];
  const setRes = await runLexi(setArgs);
  logFn && logFn(setRes.args);
  if (setRes.code !== 0) {
    sendJson(res, 500, { error: 'Failed to set prompt', details: setRes.stderr.trim() });
    return;
  }

  // Track all in-flight children so a client disconnect can kill them
  // (otherwise the lexi processes keep burning API tokens after cancel).
  const children = new Set();
  let aborted = false;
  req.on('close', () => {
    aborted = true;
    for (const c of children) {
      try { c.kill(); } catch { /* already dead */ }
    }
  });

  const runOne = (args) => new Promise((resolve) => {
    const proc = spawn(LEXI_BIN, args, {
      cwd: WORKSPACE_ROOT,
      env: { ...process.env, HOME: process.env.HOME },
    });
    children.add(proc);
    let stdout = '';
    let stderr = '';
    proc.stdout.on('data', (chunk) => { stdout += chunk.toString(); });
    proc.stderr.on('data', (chunk) => { stderr += chunk.toString(); });
    proc.on('close', (code) => {
      children.delete(proc);
      resolve({ code, stdout, stderr });
    });
    proc.on('error', (err) => {
      children.delete(proc);
      resolve({ code: 1, stdout, stderr: err.message });
    });
  });

  // Fan out: every provider runs concurrently. Order of completion doesn't
  // matter — the client keys results by provider name.
  const results = await Promise.all(
    providers.map((provider) => {
      const respArgs = buildRespondArgs(provider, {
        model: modelMap[provider],
        tools, cite, verbose, stream: false,
      });
      logFn && logFn(respArgs);
      return runOne(respArgs).then((respRes) => ({ provider, respRes }));
    })
  );

  const responses = [];
  for (const { provider, respRes } of results) {
    if (respRes.code !== 0) {
      responses.push({
        provider,
        error:
          respRes.stderr.trim() ||
          respRes.stdout.trim() ||
          `respond failed with code ${respRes.code}`,
      });
      continue;
    }
    responses.push({
      provider,
      output: respRes.stdout.trim() || '(no output)',
      stderr: respRes.stderr.trim() || '',
    });
  }

  if (aborted) {
    // Client already disconnected; nothing to send.
    return;
  }

  sendJson(res, 200, { responses });
}

// --- Respond (streaming SSE to the browser) -----------------------------

async function handleRespondStream(req, res, body, logFn) {
  const { prompt, providers, models, tools, cite, verbose } = body || {};
  if (!prompt || typeof prompt !== 'string' || !prompt.trim()) {
    sendJson(res, 400, { error: 'prompt is required' });
    return;
  }
  if (!Array.isArray(providers) || !providers.length) {
    sendJson(res, 400, { error: 'at least one provider must be selected' });
    return;
  }

  const setRes = await runLexi(['prompts', 'set', '--prompt', prompt]);
  logFn && logFn(setRes.args);
  if (setRes.code !== 0) {
    sendJson(res, 500, { error: 'Failed to set prompt', details: setRes.stderr.trim() });
    return;
  }

  res.writeHead(200, {
    'Content-Type': 'text/event-stream; charset=utf-8',
    'Cache-Control': 'no-cache, no-transform',
    'Connection': 'keep-alive',
    'X-Accel-Buffering': 'no',
  });

  // Atomic write of the whole "event: X\ndata: Y\n\n" block so concurrent
  // streams from multiple providers don't interleave inside an event.
  const sendEvent = (event, data) => {
    res.write(`event: ${event}\ndata: ${JSON.stringify(data)}\n\n`);
  };
  // Strip the trailing reset + ANSI escapes that the CLI uses for stderr "dim".
  const stripAnsi = (s) => s.replace(/\x1B\[[0-9;]*[A-Za-z]/g, '');

  const modelMap = models && typeof models === 'object' ? models : {};
  let aborted = false;
  const children = new Set();
  req.on('close', () => {
    aborted = true;
    for (const c of children) {
      try { c.kill(); } catch { /* already dead */ }
    }
  });

  // Fan out: spawn every provider's child concurrently. Events still carry
  // the provider name so the browser routes deltas to the correct tab even
  // when streams from different providers interleave.
  await Promise.all(providers.map((provider) => new Promise((resolve) => {
    if (aborted) return resolve();
    const args = buildRespondArgs(provider, {
      model: modelMap[provider], tools, cite, verbose, stream: true,
    });
    logFn && logFn(args);
    sendEvent('provider_start', { provider });

    const proc = spawn(LEXI_BIN, args, {
      cwd: WORKSPACE_ROOT,
      env: { ...process.env, HOME: process.env.HOME },
    });
    children.add(proc);
    let errBuf = '';
    proc.stdout.on('data', (chunk) => {
      sendEvent('delta', { provider, text: chunk.toString() });
    });
    proc.stderr.on('data', (chunk) => {
      // CLI dims stderr with ANSI codes; strip + split on newlines so each
      // activity indicator is a discrete event.
      const lines = stripAnsi(chunk.toString()).split(/\r?\n/);
      for (const line of lines) {
        const trimmed = line.trim();
        if (trimmed) sendEvent('activity', { provider, message: trimmed });
      }
      errBuf += chunk.toString();
    });
    proc.on('close', (code) => {
      children.delete(proc);
      sendEvent('provider_done', {
        provider,
        code,
        stderr: stripAnsi(errBuf).trim(),
      });
      resolve();
    });
    proc.on('error', (err) => {
      children.delete(proc);
      sendEvent('provider_done', { provider, code: 1, error: err.message });
      resolve();
    });
  })));

  if (!aborted) {
    sendEvent('done', {});
    res.end();
  } else {
    try { res.end(); } catch { /* already closed */ }
  }
}

// --- HTTP Server ---

const VENDOR_FILES = {
  '/vendor/marked.umd.js': {
    path: path.join(ROOT_DIR, 'node_modules', 'marked', 'lib', 'marked.umd.js'),
    type: 'application/javascript; charset=utf-8',
  },
  '/vendor/purify.min.js': {
    path: path.join(ROOT_DIR, 'node_modules', 'dompurify', 'dist', 'purify.min.js'),
    type: 'application/javascript; charset=utf-8',
  },
};

function serveVendorFile(req, res) {
  const spec = VENDOR_FILES[req.url];
  if (!spec) return false;
  try {
    const buf = fs.readFileSync(spec.path);
    res.writeHead(200, {
      'Content-Type': spec.type,
      'Cache-Control': 'public, max-age=86400',
    });
    res.end(buf);
  } catch (err) {
    res.writeHead(404, { 'Content-Type': 'text/plain' });
    res.end(`Vendor asset not found: ${err.message}. Did you run \`npm install\`?`);
  }
  return true;
}

const server = http.createServer((req, res) => {
  if (req.method === 'GET' && (req.url === '/' || req.url === '/index.html')) {
    serveIndex(res);
    return;
  }

  if (req.method === 'GET' && req.url.startsWith('/vendor/')) {
    if (serveVendorFile(req, res)) return;
  }

  if (req.url.startsWith('/api/prompts')) {
    handlePrompt(req, res, (args) => {
      res.setHeader('X-Lexi-Command', args.join(' '));
    });
    return;
  }

  // --- Respond route ---
  if (req.method === 'POST' && req.url === '/api/respond') {
    const commandLog = [];
    const sanitize = (text = '') => text.replace(/\r?\n/g, '\\n');
    const logFn = (args) => {
      if (!Array.isArray(args)) return;
      commandLog.push(args.map((a) => sanitize(String(a))).join(' '));
      res.setHeader('X-Lexi-Command', commandLog.join(' && '));
    };
    let body = '';
    req.on('data', (chunk) => {
      body += chunk.toString();
      if (body.length > 2_000_000) {
        sendJson(res, 413, { error: 'Request too large' });
        req.destroy();
      }
    });
    req.on('end', () => {
      let parsed = {};
      try {
        parsed = JSON.parse(body || '{}');
      } catch {
        sendJson(res, 400, { error: 'Invalid JSON body' });
        return;
      }
      Promise.resolve()
        .then(() => handleRespond(req, res, parsed, logFn))
        .catch((err) => {
          console.error('[lexi-gui] respond handler error', err);
          sendJson(res, 500, { error: 'Internal error', details: err.message });
        });
    });
    return;
  }

  if (req.method === 'GET' && req.url.startsWith('/api/respond/options')) {
    const url = new URL(req.url, `http://localhost:${PORT}`);
    const providersParam = url.searchParams.get('providers') || '';
    handleRespondOptions(res, providersParam);
    return;
  }

  if (req.method === 'POST' && req.url === '/api/respond/stream') {
    const commandLog = [];
    const sanitize = (text = '') => text.replace(/\r?\n/g, '\\n');
    const logFn = (args) => {
      if (!Array.isArray(args)) return;
      commandLog.push(args.map((a) => sanitize(String(a))).join(' '));
      // header may already be sent by the time we get here; ignore failures.
      try { res.setHeader('X-Lexi-Command', commandLog.join(' && ')); } catch { /* ignore */ }
    };
    let body = '';
    req.on('data', (chunk) => {
      body += chunk.toString();
      if (body.length > 2_000_000) {
        sendJson(res, 413, { error: 'Request too large' });
        req.destroy();
      }
    });
    req.on('end', () => {
      let parsed = {};
      try {
        parsed = JSON.parse(body || '{}');
      } catch {
        sendJson(res, 400, { error: 'Invalid JSON body' });
        return;
      }
      Promise.resolve()
        .then(() => handleRespondStream(req, res, parsed, logFn))
        .catch((err) => {
          console.error('[lexi-gui] respond stream handler error', err);
          try { sendJson(res, 500, { error: 'Internal error', details: err.message }); } catch { /* response may be open */ }
        });
    });
    return;
  }

  // --- Models management routes (must come before generic /api/models) ---
  if (req.method === 'GET' && req.url.startsWith('/api/models/default')) {
    const url = new URL(req.url, `http://localhost:${PORT}`);
    const provider = url.searchParams.get('provider');
    const commandLog = [];
    const logFn = (args) => { if (Array.isArray(args)) { commandLog.push(args.join(' ')); res.setHeader('X-Lexi-Command', commandLog.join(' && ')); } };
    handleModelsDefault(res, provider, logFn);
    return;
  }

  if (req.method === 'GET' && req.url.startsWith('/api/models/aliases')) {
    const url = new URL(req.url, `http://localhost:${PORT}`);
    const provider = url.searchParams.get('provider');
    const commandLog = [];
    const logFn = (args) => { if (Array.isArray(args)) { commandLog.push(args.join(' ')); res.setHeader('X-Lexi-Command', commandLog.join(' && ')); } };
    handleModelsAliases(res, provider, logFn);
    return;
  }

  if (req.method === 'GET' && req.url.startsWith('/api/models/hosted')) {
    const url = new URL(req.url, `http://localhost:${PORT}`);
    const provider = url.searchParams.get('provider');
    const commandLog = [];
    const logFn = (args) => { if (Array.isArray(args)) { commandLog.push(args.join(' ')); res.setHeader('X-Lexi-Command', commandLog.join(' && ')); } };
    handleModelsHosted(res, provider, logFn);
    return;
  }

  if (req.method === 'POST' && req.url.startsWith('/api/models/')) {
    const commandLog = [];
    const sanitize = (text = '') => text.replace(/\r?\n/g, '\\n');
    const logFn = (args) => {
      if (!Array.isArray(args)) return;
      commandLog.push(args.map((a) => sanitize(String(a))).join(' '));
      res.setHeader('X-Lexi-Command', commandLog.join(' && '));
    };
    let body = '';
    req.on('data', (chunk) => {
      body += chunk.toString();
      if (body.length > 1e6) req.destroy();
    });
    req.on('end', () => {
      let parsed = {};
      try {
        parsed = JSON.parse(body || '{}');
      } catch {
        sendJson(res, 400, { error: 'Invalid JSON body' });
        return;
      }
      let handler;
      if (req.url === '/api/models/default') handler = () => handleSetModelsDefault(res, parsed, logFn);
      else if (req.url === '/api/models/aliases') handler = () => handleSetModelAlias(res, parsed, logFn);
      else if (req.url === '/api/models/aliases/delete') handler = () => handleDeleteModelAlias(res, parsed, logFn);
      else if (req.url === '/api/models/add') handler = () => handleAddModel(res, parsed, logFn);
      else if (req.url === '/api/models/set') handler = () => handleSetModel(res, parsed, logFn);
      else if (req.url === '/api/models/delete') handler = () => handleDeleteModel(res, parsed, logFn);
      else {
        sendJson(res, 404, { error: 'Not Found' });
        return;
      }
      Promise.resolve().then(handler).catch((err) => {
        console.error('[lexi-gui] models handler error', err);
        sendJson(res, 500, { error: 'Internal error', details: err.message });
      });
    });
    return;
  }

  if (req.method === 'GET' && req.url.startsWith('/api/models')) {
    const url = new URL(req.url, `http://localhost:${PORT}`);
    const provider = url.searchParams.get('provider');
    handleModelsList(res, provider, (args) => {
      res.setHeader('X-Lexi-Command', args.join(' '));
    });
    return;
  }

  // --- API definition routes ---
  if (req.method === 'GET' && req.url.startsWith('/api/definitions/raw/')) {
    const name = decodeURIComponent(req.url.replace('/api/definitions/raw/', '').replace(/\/$/, ''));
    const commandLog = [];
    const logFn = (args) => { if (Array.isArray(args)) { commandLog.push(args.join(' ')); res.setHeader('X-Lexi-Command', commandLog.join(' && ')); } };
    handleApiRaw(res, name, logFn);
    return;
  }

  if (req.method === 'GET' && req.url.startsWith('/api/definitions/validate/')) {
    const name = decodeURIComponent(req.url.replace('/api/definitions/validate/', '').replace(/\/$/, ''));
    const commandLog = [];
    const logFn = (args) => { if (Array.isArray(args)) { commandLog.push(args.join(' ')); res.setHeader('X-Lexi-Command', commandLog.join(' && ')); } };
    handleApiValidate(res, name, logFn);
    return;
  }

  if (req.method === 'GET' && req.url.startsWith('/api/definitions/')) {
    const name = decodeURIComponent(req.url.replace('/api/definitions/', '').replace(/\/$/, ''));
    const commandLog = [];
    const logFn = (args) => { if (Array.isArray(args)) { commandLog.push(args.join(' ')); res.setHeader('X-Lexi-Command', commandLog.join(' && ')); } };
    handleApiShow(res, name, logFn);
    return;
  }

  if (req.method === 'GET' && (req.url === '/api/definitions' || req.url === '/api/definitions/')) {
    const commandLog = [];
    const logFn = (args) => { if (Array.isArray(args)) { commandLog.push(args.join(' ')); res.setHeader('X-Lexi-Command', commandLog.join(' && ')); } };
    handleApiList(res, logFn);
    return;
  }

  if (req.method === 'POST' && req.url.startsWith('/api/definitions')) {
    const commandLog = [];
    const sanitize = (text = '') => text.replace(/\r?\n/g, '\\n');
    const logFn = (args) => {
      if (!Array.isArray(args)) return;
      commandLog.push(args.map((a) => sanitize(String(a))).join(' '));
      res.setHeader('X-Lexi-Command', commandLog.join(' && '));
    };
    let body = '';
    req.on('data', (chunk) => {
      body += chunk.toString();
      if (body.length > 1e6) req.destroy();
    });
    req.on('end', () => {
      let parsed = {};
      try {
        parsed = JSON.parse(body || '{}');
      } catch {
        sendJson(res, 400, { error: 'Invalid JSON body' });
        return;
      }
      let handler;
      if (req.url === '/api/definitions/create') handler = () => handleApiCreate(res, parsed, logFn);
      else if (req.url === '/api/definitions/edit') handler = () => handleApiEdit(res, parsed, logFn);
      else if (req.url === '/api/definitions/delete') handler = () => handleApiDelete(res, parsed, logFn);
      else if (req.url === '/api/definitions/save-raw') handler = () => handleApiSaveRaw(res, parsed, logFn);
      else if (req.url === '/api/definitions/preview') handler = () => handleApiPreview(res, parsed, logFn);
      else {
        sendJson(res, 404, { error: 'Not Found' });
        return;
      }
      Promise.resolve().then(handler).catch((err) => {
        console.error('[lexi-gui] api definition handler error', err);
        sendJson(res, 500, { error: 'Internal error', details: err.message });
      });
    });
    return;
  }

  // --- Provider routes ---
  if (req.method === 'POST' && req.url === '/api/providers/api-key') {
    const commandLog = [];
    const sanitize = (text = '') => text.replace(/\r?\n/g, '\\n');
    const logFn = (args) => {
      if (!Array.isArray(args)) return;
      commandLog.push(args.map((a) => sanitize(String(a))).join(' '));
      res.setHeader('X-Lexi-Command', commandLog.join(' && '));
    };
    let body = '';
    req.on('data', (chunk) => {
      body += chunk.toString();
      if (body.length > 1e6) req.destroy();
    });
    req.on('end', () => {
      let parsed = {};
      try {
        parsed = JSON.parse(body || '{}');
      } catch {
        sendJson(res, 400, { error: 'Invalid JSON body' });
        return;
      }
      Promise.resolve()
        .then(() => handleApiKey(req, res, parsed, logFn))
        .catch((err) => {
          console.error('[lexi-gui] api-key handler error', err);
          sendJson(res, 500, { error: 'Internal error', details: err.message });
        });
    });
    return;
  }

  if (req.method === 'GET' && (req.url === '/api/providers' || req.url === '/api/providers/')) {
    handleProviders(res);
    return;
  }

  if (req.method === 'GET' && req.url === '/api/providers/config-source') {
    const candidates = [USER_PROVIDERS_CONFIG, REPO_PROVIDERS_CONFIG];
    const found = candidates.find((p) => p && fs.existsSync(p));
    if (!found) {
      sendJson(res, 404, { error: 'providers-config.yaml not found.' });
      return;
    }
    try {
      const text = fs.readFileSync(found, 'utf8');
      sendJson(res, 200, { source: found, text });
    } catch (err) {
      sendJson(res, 500, { error: `Failed to read providers-config.yaml: ${err.message}` });
    }
    return;
  }

  if (req.method === 'GET' && req.url === '/api/providers/config-source/view') {
    const candidates = [USER_PROVIDERS_CONFIG, REPO_PROVIDERS_CONFIG];
    const found = candidates.find((p) => p && fs.existsSync(p));
    if (!found) {
      res.writeHead(404, { 'Content-Type': 'text/plain; charset=utf-8' });
      res.end('providers-config.yaml not found.');
      return;
    }
    let text;
    try {
      text = fs.readFileSync(found, 'utf8');
    } catch (err) {
      res.writeHead(500, { 'Content-Type': 'text/plain; charset=utf-8' });
      res.end(`Failed to read providers-config.yaml: ${err.message}`);
      return;
    }
    const escape = (s) => String(s).replace(/[&<>"']/g, (c) => ({
      '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;'
    }[c]));
    const html = `<!doctype html>
<html>
<head>
  <meta charset="utf-8">
  <title>${escape(found)}</title>
  <style>
    html, body { margin: 0; padding: 0; height: 100%; background: #0f1424; color: #e6e8ef;
      font-family: ui-sans-serif, system-ui, -apple-system, Segoe UI, Roboto, sans-serif; }
    header { padding: 10px 14px; border-bottom: 1px solid #2b3146; font-weight: 700;
      font-size: 0.9rem; word-break: break-all; }
    main { padding: 12px 16px; overflow: auto; height: calc(100% - 44px); box-sizing: border-box; }
    pre { margin: 0; white-space: pre; font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
      font-size: 0.85rem; }
  </style>
</head>
<body>
  <header>${escape(found)}</header>
  <main><pre>${escape(text)}</pre></main>
</body>
</html>`;
    res.writeHead(200, { 'Content-Type': 'text/html; charset=utf-8' });
    res.end(html);
    return;
  }

  // --- Config routes ---
  if (req.method === 'GET' && (req.url === '/api/config' || req.url === '/api/config/')) {
    handleGetConfig(res);
    return;
  }

  if (req.method === 'POST' && (req.url === '/api/config' || req.url === '/api/config/')) {
    const commandLog = [];
    const sanitize = (text = '') => text.replace(/\r?\n/g, '\\n');
    const logFn = (args) => {
      if (!Array.isArray(args)) return;
      commandLog.push(args.map((a) => sanitize(String(a))).join(' '));
      res.setHeader('X-Lexi-Command', commandLog.join(' && '));
    };
    let body = '';
    req.on('data', (chunk) => {
      body += chunk.toString();
      if (body.length > 1e6) req.destroy();
    });
    req.on('end', () => {
      let parsed = {};
      try {
        parsed = JSON.parse(body || '{}');
      } catch {
        sendJson(res, 400, { error: 'Invalid JSON body' });
        return;
      }
      Promise.resolve()
        .then(() => handleSetConfig(req, res, parsed, logFn))
        .catch((err) => {
          console.error('[lexi-gui] config handler error', err);
          sendJson(res, 500, { error: 'Internal error', details: err.message });
        });
    });
    return;
  }

  if (req.method === 'OPTIONS') {
    res.writeHead(204, {
      'Access-Control-Allow-Origin': '*',
      'Access-Control-Allow-Methods': 'POST, GET, OPTIONS',
      'Access-Control-Allow-Headers': 'Content-Type',
    });
    res.end();
    return;
  }

  res.writeHead(404, { 'Content-Type': 'text/plain' });
  res.end('Not Found');
});

server.listen(PORT, () => {
  console.log(`GUI available at http://localhost:${PORT}`);
});
