# Owned HTTPS push fixture

These public test-only DER certificate and PKCS#8 key files identify
`push.example.test`. They are never production credentials. The provider binds only
an ephemeral loopback port, and only binaries with `test-failpoints` can trust this
certificate or replace the provider's DNS destination. Default-feature binaries
ignore the fixture environment variables.
