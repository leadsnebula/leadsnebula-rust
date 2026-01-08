# Local HTTPS Setup for Passkey Testing

Passkeys require HTTPS to work. This guide will help you set up HTTPS for local development.

## Quick Setup (Recommended)

### Step 1: Install mkcert

```bash
# On Ubuntu/Debian
sudo apt install mkcert

# On macOS
brew install mkcert

# On Windows (with Chocolatey)
choco install mkcert
```

### Step 2: Install Local CA

```bash
mkcert -install
```

This installs a local Certificate Authority that your browser will trust.

### Step 3: Generate SSL Certificates

```bash
# Navigate to the frontend directory
cd /home/badinoff/projects/leadsNebula/frontend

# Generate certificates for localhost
mkcert -key-file localhost-key.pem -cert-file localhost.pem localhost 127.0.0.1 ::1
```

This creates:
- `localhost-key.pem` (private key)
- `localhost.pem` (certificate)

**Important**: Add these files to `.gitignore`:
```bash
echo "localhost*.pem" >> .gitignore
```

### Step 4: Configure Frontend (Next.js)

Update `package.json` to add an HTTPS dev script:

```json
"scripts": {
  "dev": "next dev",
  "dev:https": "next dev --experimental-https --experimental-https-key ./localhost-key.pem --experimental-https-cert ./localhost.pem"
}
```

Or use a custom server (see below).

### Step 5: Configure Backend

Add to your `.env.local` in the `rust` directory:

```bash
WEBAUTHN_LOCAL_HTTPS=https://localhost:3000
```

### Step 6: Start Services

1. **Frontend (HTTPS)**:
   ```bash
   cd /home/badinoff/projects/leadsNebula/frontend
   npm run dev:https
   ```
   Access at: `https://localhost:3000`

2. **Backend**:
   ```bash
   cd /home/badinoff/projects/leadsNebula/rust
   cargo run --bin leadsnebula-api
   ```

## Alternative: Custom Next.js Server with HTTPS

If the experimental HTTPS flag doesn't work, create a custom server:

1. Install dependencies:
   ```bash
   cd frontend
   npm install --save-dev https
   ```

2. Create `server.js`:
   ```javascript
   const { createServer } = require('https');
   const { parse } = require('url');
   const next = require('next');
   const fs = require('fs');
   const path = require('path');

   const dev = process.env.NODE_ENV !== 'production';
   const hostname = 'localhost';
   const port = 3000;

   const app = next({ dev, hostname, port });
   const handle = app.getRequestHandler();

   const httpsOptions = {
     key: fs.readFileSync(path.join(__dirname, 'localhost-key.pem')),
     cert: fs.readFileSync(path.join(__dirname, 'localhost.pem')),
   };

   app.prepare().then(() => {
     createServer(httpsOptions, async (req, res) => {
       try {
         const parsedUrl = parse(req.url, true);
         await handle(req, res, parsedUrl);
       } catch (err) {
         console.error('Error occurred handling', req.url, err);
         res.statusCode = 500;
         res.end('internal server error');
       }
     }).listen(port, (err) => {
       if (err) throw err;
       console.log(`> Ready on https://${hostname}:${port}`);
     });
   });
   ```

3. Update `package.json`:
   ```json
   "scripts": {
     "dev:https": "node server.js"
   }
   ```

## Verification

1. Open `https://localhost:3000` in your browser
2. You should see a valid SSL certificate (no warnings)
3. Try registering a passkey - it should work now!

## Troubleshooting

### Certificate Not Trusted
- Make sure you ran `mkcert -install`
- Restart your browser after installing the CA

### Port Already in Use
- Change the port in `server.js` or use `-p 3001` with Next.js

### Backend Still Using HTTP
- Check that `WEBAUTHN_LOCAL_HTTPS=https://localhost:3000` is set in `.env.local`
- Restart the backend server

## Production

Production is already configured to use HTTPS automatically. This setup only affects local development.

## Notes

- The WebAuthn security requirement is enforced by the browser/passkey manager, not our server code
- This is a security feature that cannot be bypassed
- Production will always use HTTPS and doesn't need this configuration
