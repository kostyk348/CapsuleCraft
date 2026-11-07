# Release signing key (committed on purpose)

`capsulecraft-release.keystore` signs every release APK so that sideloaded
upgrades install over the previous build without an uninstall. The project
has no GitHub Actions secret storage available, so the key ships in the repo
with its password in the workflow — public by necessity.

**Treat the APK signature as build identity, not as security.** Anyone who
clones this repository can build a same-signature APK. If the project ever
gains secret storage, move the keystore to `ANDROID_KEYSTORE_BASE64` /
`ANDROID_KEYSTORE_PASSWORD` / `ANDROID_KEY_ALIAS` repo secrets and rotate:
the release job already prefers the secrets branch.

- alias: `capsulecraft`
- store/key password: `c88f596eab12ed8a2d11b84904da1d26`
- certificate SHA-256: `E1:C5:10:79:33:97:79:65:C9:BA:42:B0:BD:73:55:FB:F2:33:4F:51:65:16:54:71:1A:77:D0:65:1F:5D:8C:94`
- validity: 10950 days (30 years), RSA-2048, PKCS12
