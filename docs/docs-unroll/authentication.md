---
sidebar_position: 14
title: Authentication
description: How to authenticate with the Oite package registry using device authorization or tokens.
keywords: [authentication, login, logout, token, credentials, device auth]
---

# Authentication

Authentication is required to publish packages, yank versions, and manage your packages on the registry.

## Login

```bash
unroll login
```

### Device Authorization Flow

The default authentication method is device authorization:

1. Unroll requests a device code from the registry
2. A browser window opens to the authorization page
3. You enter the displayed user code and authorize
4. Unroll polls the registry until authorization completes
5. Your token is saved locally

```
Logging in to https://registry.oite.org/api/v1...

Please open this URL in your browser:
  https://registry.oite.org/auth/device?code=ABCD-1234

Enter this code: ABCD-1234

Waiting for authorization...
Logged in as: username
Token saved to ~/.unroll/credentials
```

### Manual Token Entry

If device authorization is unavailable (e.g., headless environments), unroll falls back to manual token entry:

1. Visit the registry website
2. Generate an API token in your account settings
3. Paste the token when prompted

## Logout

```bash
unroll logout
```

Clears the stored token from `~/.unroll/credentials`.

## Token Storage

Tokens are stored in `~/.unroll/credentials`:

```
https://registry.oite.org/api/v1 = "oat_abc123def456ghi789"
```

Format: `registry_url = "token"` (one entry per line).

### File Permissions

The credentials file should be readable only by you:

```bash
chmod 600 ~/.unroll/credentials
```

## Environment Variable

For CI/CD or when you don't want to use the credentials file, set the `UNROLL_TOKEN` environment variable:

```bash
export UNROLL_TOKEN="oat_abc123def456ghi789"
unroll publish
```

The environment variable takes priority over the credentials file.

## Token Configuration in Config File

You can also reference tokens in `~/.unroll/config.toml`:

```toml
[registry]
default = "https://registry.oite.org/api/v1"

# Direct token
token = "oat_abc123def456ghi789"

# Or reference an environment variable
token = "env:UNROLL_TOKEN"
```

## Priority Order

When multiple token sources are available, the priority is (highest first):

1. `UNROLL_TOKEN` environment variable
2. `~/.unroll/credentials` file (matched by registry URL)
3. `~/.unroll/config.toml` `token` field

## Verifying Authentication

After login, unroll validates your token by calling the `/me` endpoint:

```
Logged in as: username
```

If the token is invalid, you'll see:

```
Error: token validation failed
```

## CI/CD Usage

### GitHub Actions

```yaml
- name: Publish
  env:
    UNROLL_TOKEN: ${{ secrets.UNROLL_TOKEN }}
  run: unroll publish
```

Store your token as a GitHub repository secret:
1. Go to Settings > Secrets and variables > Actions
2. Add a new repository secret named `UNROLL_TOKEN`
3. Paste your API token

### Other CI Providers

Set the `UNROLL_TOKEN` environment variable in your CI configuration:

```bash
# GitLab CI
variables:
  UNROLL_TOKEN: $REGISTRY_TOKEN

# CircleCI
environment:
  UNROLL_TOKEN: your-token
```

## Custom Registry

When using a custom registry, tokens are matched by URL:

```bash
# Set custom registry
export UNROLL_REGISTRY="https://registry.internal.company.com/api/v1"

# Login to custom registry
unroll login

# Token is stored for this specific registry URL
```

The credentials file can store tokens for multiple registries:

```
https://registry.oite.org/api/v1 = "oat_public_token"
https://registry.internal.company.com/api/v1 = "company_token"
```

## Security

- Never commit tokens to version control
- Use environment variables in CI/CD
- Rotate tokens periodically
- Use `unroll logout` when done on shared machines
- The credentials file should have `600` permissions
