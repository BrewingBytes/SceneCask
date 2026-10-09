# Google OIDC protocol fixtures

`tests/google.rs` runs a local OpenID Connect provider standing in for Google: discovery,
JWKS and an authorization-code token endpoint that enforces client authentication, the
registered redirect URI, single-use codes and the S256 PKCE challenge. It signs ID tokens
with the keys in this directory.

`signing-key.pem` and `foreign-key.pem` are throwaway 2048-bit RSA keys (PKCS#1) generated
for these tests only with `openssl genrsa -traditional 2048`. They protect nothing, are never
trusted outside the fixture, and must not be reused. `foreign-key.pem` signs tokens that
claim the fixture's key ID, to prove signature validation. The tests never contact Google;
real Google sign-in is a manual release smoke check with deployment credentials.
