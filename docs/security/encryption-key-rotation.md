# Encryption Key Rotation

This project encrypts sensitive fields at rest using AES-256-GCM with
`ENCRYPTION_KEY` (32 bytes).

## Encrypted fields

- `merchants.apiKeyHash`
- `refresh_tokens.tokenHash`

## Rotation process

1. **Prepare new key**
   - Generate a new 32-byte key and store it in secrets manager as
     `ENCRYPTION_KEY_NEXT`.
2. **Deploy dual-read migration build**
   - Run a one-off script that reads all encrypted rows, decrypts with current
     `ENCRYPTION_KEY`, and re-saves using `ENCRYPTION_KEY_NEXT`.
   - Perform this in batches to avoid long transactions.
3. **Switch active key**
   - Promote `ENCRYPTION_KEY_NEXT` to `ENCRYPTION_KEY` in all environments.
4. **Verify**
   - Run a smoke test that reads a sample of merchants and refresh tokens and
     confirms decrypt success.
   - Check logs for `[SECURITY_EVENT] Failed to decrypt`.
5. **Retire old key**
   - Remove access to previous key material after verification window closes.

## Operational safeguards

- Never log decrypted field values.
- Treat decryption failures as security events and investigate immediately.
- Keep key rotation limited to least-privilege operators and audited change
  windows.
