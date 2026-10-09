# Signing on macOS

The Keychain grants "Always Allow" to a program by its code signature. The linker signs each build ad hoc, so every new build is a new program to macOS and the Keychain asks for the login password again. A stable signature fixes that: the Keychain then asks once.

## Development builds

On macOS, `.cargo/config.toml` sets `scripts/run-signed.sh` as the cargo runner, so `cargo run` goes through it. When the binary is `zefiro`, the script signs it with a stable identity and the identifier `dev.zefiro`, then runs it with the given arguments. Test binaries and every other binary run unchanged and unsigned.

The identity is `ZEFIRO_SIGN_IDENTITY` when set, else the first "Apple Development" identity that `security find-identity -v -p codesigning` lists. To pick one, set the variable to its SHA-1 hash or its full name:

```
security find-identity -v -p codesigning
export ZEFIRO_SIGN_IDENTITY="Apple Development: Name (TEAMID)"
```

An Apple Development certificate comes free with an Apple ID: Xcode → Settings → Accounts → Manage Certificates → "+" → Apple Development. With no identity, the script runs the binary unsigned and says so on stderr; the Keychain then asks at every new build, as before.

To check a build: `codesign -dv target/debug/zefiro` shows `Identifier=dev.zefiro` and, with `-dvv`, `Authority=Apple Development: …`.

## Release builds

The runner only serves `cargo run`. A build given to anyone else needs a Developer ID Application signature with the hardened runtime, and notarization; an Apple Development signature runs only on the developer's own machines.
