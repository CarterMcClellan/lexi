const { app, BrowserWindow, ipcMain } = require('electron');
const { spawn } = require('child_process');
const path = require('path');
const fs = require('fs');
const os = require('os');

let server;
let mainWindow;

// Determine the bundled CLI path
function getCliPath() {
  if (app.isPackaged) {
    // Production: CLI is bundled in resources
    const resourcesPath = process.resourcesPath;
    const platform = os.platform();

    if (platform === 'win32') {
      return path.join(resourcesPath, 'cli', 'lexi.exe');
    } else {
      return path.join(resourcesPath, 'cli', 'lexi');
    }
  } else {
    // Development: use local CLI
    return path.join(__dirname, '..', '..', '..', 'lexi-cli', 'cli');
  }
}

// Determine Python path for CLI
function getPythonPath() {
  if (app.isPackaged) {
    // Production: bundled Python
    const resourcesPath = process.resourcesPath;
    const platform = os.platform();

    if (platform === 'win32') {
      return path.join(resourcesPath, 'python', 'python.exe');
    } else {
      return path.join(resourcesPath, 'python', 'bin', 'python');
    }
  } else {
    // Development: system Python
    return 'python3';
  }
}

function createWindow() {
  mainWindow = new BrowserWindow({
    width: 1200,
    height: 800,
    minWidth: 800,
    minHeight: 600,
    show: false, // Don't show until ready
    webPreferences: {
      nodeIntegration: false,
      contextIsolation: true,
      preload: path.join(__dirname, 'preload.js')
    }
  });

  // Handle window ready
  mainWindow.once('ready-to-show', () => {
    mainWindow.show();
    console.log('Window shown');
  });

  // Load the GUI with error handling
  mainWindow.loadURL('http://localhost:3000').catch((err) => {
    console.error('Failed to load URL:', err);

    // Fallback: try loading a simple error page
    mainWindow.loadURL(`data:text/html;charset=utf-8,
      <html>
        <body style="font-family: Arial; padding: 20px; text-align: center;">
          <h1>Lexi Starting...</h1>
          <p>Server is starting up. Please wait...</p>
          <script>
            setTimeout(() => {
              window.location.href = 'http://localhost:3000';
            }, 3000);
          </script>
        </body>
      </html>
    `);
  });

  // Open DevTools in development
  if (!app.isPackaged) {
    mainWindow.webContents.openDevTools();
  }

  // Handle window closed
  mainWindow.on('closed', () => {
    mainWindow = null;
  });

  // Handle navigation errors
  mainWindow.webContents.on('did-fail-load', (event, errorCode, errorDescription, validatedURL) => {
    console.error(`Failed to load ${validatedURL}: ${errorDescription} (${errorCode})`);

    // Retry loading after a delay
    setTimeout(() => {
      console.log('Retrying connection...');
      mainWindow.loadURL('http://localhost:3000');
    }, 2000);
  });
}

function startServer() {
  return new Promise((resolve, reject) => {
    const http = require('http');
    const serverPath = path.join(__dirname, '..', 'server', 'index.js');

    // Use the standalone server module
    const serverModule = require(serverPath);

    // If the server module exports a listen function or server instance, use it
    // Otherwise the require itself starts the server
    const PORT = 3000;

    // Wait for server to be ready
    const checkServer = (retries = 0) => {
      if (retries > 30) {
        reject(new Error('Server failed to start after 30 attempts'));
        return;
      }
      const req = http.get(`http://localhost:${PORT}`, (res) => {
        resolve();
      });
      req.on('error', () => {
        setTimeout(() => checkServer(retries + 1), 1000);
      });
    };

    setTimeout(() => checkServer(), 1000);
  });
}

// App event handlers
app.whenReady().then(async () => {
  try {
    console.log('App ready, starting server...');
    console.log('Resource path:', process.resourcesPath);
    console.log('CLI path:', getCliPath());
    console.log('App packaged:', app.isPackaged);

    await startServer();
    console.log('Server started successfully, creating window...');
    createWindow();
  } catch (error) {
    console.error('Failed to start server:', error);
    console.error('Error details:', error.stack);

    // Create a simple error window instead of quitting
    const errorWindow = new BrowserWindow({
      width: 600,
      height: 400,
      webPreferences: { nodeIntegration: true, contextIsolation: false }
    });

    errorWindow.loadURL(`data:text/html;charset=utf-8,
      <html>
        <head><title>Lexi - Startup Error</title></head>
        <body style="font-family: Arial; padding: 20px; background: #f5f5f5;">
          <h2>Lexi Startup Error</h2>
          <p><strong>Error:</strong> ${error.message}</p>
          <p><strong>Details:</strong></p>
          <pre style="background: #fff; padding: 10px; border-radius: 4px; overflow: auto;">${error.stack || 'No stack trace available'}</pre>
          <h3>Debug Information:</h3>
          <ul>
            <li>App Packaged: ${app.isPackaged}</li>
            <li>Resource Path: ${process.resourcesPath || 'Not available'}</li>
            <li>CLI Path: ${getCliPath()}</li>
            <li>Node Version: ${process.version}</li>
          </ul>
          <p>Please try running the CLI directly first: <code>lexi help</code></p>
        </body>
      </html>
    `);
  }

  app.on('activate', () => {
    if (BrowserWindow.getAllWindows().length === 0) {
      createWindow();
    }
  });
});

app.on('window-all-closed', () => {
  if (server) {
    if (server.close) {
      server.close();
    } else if (server.kill) {
      server.kill();
    }
  }

  if (process.platform !== 'darwin') {
    app.quit();
  }
});

app.on('before-quit', () => {
  if (server) {
    if (server.close) {
      server.close();
    } else if (server.kill) {
      server.kill();
    }
  }
});

// Handle CLI commands from renderer
ipcMain.handle('run-cli-command', async (event, args) => {
  return new Promise((resolve) => {
    const cliPath = getCliPath();
    const pythonPath = getPythonPath();

    const proc = spawn(pythonPath, [cliPath, ...args], {
      stdio: ['ignore', 'pipe', 'pipe']
    });

    let stdout = '';
    let stderr = '';

    proc.stdout.on('data', (data) => {
      stdout += data.toString();
    });

    proc.stderr.on('data', (data) => {
      stderr += data.toString();
    });

    proc.on('close', (code) => {
      resolve({ code, stdout, stderr });
    });
  });
});
