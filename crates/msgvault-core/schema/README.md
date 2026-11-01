# Vendored msgvault schema

`schema.sql` and `schema_sqlite.sql` are verbatim copies of
`internal/store/schema.sql` and `internal/store/schema_sqlite.sql` from
[msgvault](https://github.com/wesm/msgvault) at commit `c0e0afc8`, distributed
under the MIT license in `LICENSE-msgvault`.

They are used only to build synthetic demo/test archives, so the SQLite reader
is exercised against the real production schema. The application never writes
to a user's archive. Refresh them with `just vendor-schema <msgvault checkout>`.
