# Подписи релизов и установщиков

Каждый релиз CapsuleCraft подписан. Проверить артефакт может любой — ключи и сертификаты
не нужны, нужен только `cosign` (или `gpg`).

## Что публикуется в релизе

| Файл | Что это |
|---|---|
| `SHA256SUMS.txt` | контрольные суммы всех артефактов релиза |
| `SHA256SUMS.txt.sig` + `SHA256SUMS.txt.pem` | **keyless-подпись cosign** поверх `SHA256SUMS.txt` (запись в публичном прозрачном логе Rekor, привязана к workflow `release.yml`) |
| `SHA256SUMS.txt.asc` | **GPG-подпись** того же файла (если в репозитории задан секрет `GPG_PRIVATE_KEY`) |

Подписаны именно контрольные суммы: подпись покрывает весь набор артефактов сразу.

## Как проверить

```bash
# 1) скачать суммы, подпись и сертификат
TAG=v2.12.5
BASE=https://github.com/kostyk348/CapsuleCraft/releases/download/$TAG
curl -sSfLO $BASE/SHA256SUMS.txt
curl -sSfLO $BASE/SHA256SUMS.txt.sig
curl -sSfLO $BASE/SHA256SUMS.txt.pem

# 2) проверить подпись (нужен cosign: https://docs.sigstore.dev/cosign/installation/)
cosign verify-blob \
  --certificate SHA256SUMS.txt.pem \
  --signature SHA256SUMS.txt.sig \
  --certificate-identity-regexp "https://github.com/kostyk348/CapsuleCraft/.github/workflows/release.yml@.*" \
  --certificate-oidc-issuer "https://token.actions.githubusercontent.com" \
  SHA256SUMS.txt

# 3) проверить сам установщик
sha256sum -c --ignore-missing SHA256SUMS.txt   # должен совпасть твой файл
```

Всё это одной командой: `scripts/verify-release.sh v2.12.5 CapsuleCraft_2.12.5_amd64.AppImage`

## Установщики по платформам

| Платформа | Артефакт | Что даёт |
|---|---|---|
| Windows | `CapsuleCraft_<ver>_setup.exe` (NSIS) | установка в меню «Пуск», деинсталлятор |
| Windows | `CapsuleCraft_native_<ver>_x64.zip` | портативная версия без установки |
| Linux | `CapsuleCraft_<ver>_amd64.deb` | apt/dpkg |
| Linux | `CapsuleCraft_<ver>_x86_64.rpm` | dnf/zypper |
| Linux | `CapsuleCraft_<ver>_amd64.AppImage` | без установки, со вложенными библиотеками |
| macOS | `CapsuleCraft_<ver>_macos_universal.dmg` | drag-to-Applications, arm64 + x64 |
| Android | `capsulecraft-android-*.zip` | APK-пакет |

## Подпись кода: что уже включено, а что требует твоего сертификата

**Уже работает без каких-либо секретов**

- cosign keyless-подпись контрольных сумм (OIDC GitHub Actions + Rekor) — проверяемо кем угодно.
- macOS-сборка подписывается **ad-hoc** (`codesign -s -`): не блокируется при локальном запуске,
  но Gatekeeper покажет «неизвестный разработчик» при первом открытии из интернета.

**Включается автоматически, как только появятся секреты** (шаг пропускается молча, если секрета нет)

| Секрет | Что включает | Где брать |
|---|---|---|
| `WINDOWS_CERT_PFX_BASE64` + `WINDOWS_CERT_PASSWORD` | Authenticode-подпись `capsuleos-native.exe` и `setup.exe` (доверие SmartScreen) | OV/EV-сертификат подписи кода (например, Sectigo/DigiCert) либо Azure Trusted Signing |
| `APPLE_CERT_P12_BASE64` + `APPLE_CERT_PASSWORD` + `APPLE_ID` + `APPLE_TEAM_ID` + `APPLE_APP_PASSWORD` | Developer ID-подпись `.app`/`.dmg` + нотаризация и stapling | Apple Developer Program (99 $/год) |
| `GPG_PRIVATE_KEY` (+ `GPG_PASSPHRASE`) | `SHA256SUMS.txt.asc` | свой GPG-ключ |

Задать секреты:

```bash
gh secret set WINDOWS_CERT_PFX_BASE64 -R kostyk348/CapsuleCraft < cert.pfx.base64
gh secret set WINDOWS_CERT_PASSWORD  -R kostyk348/CapsuleCraft
gh secret set APPLE_CERT_P12_BASE64  -R kostyk348/CapsuleCraft < cert.p12.base64
gh secret set APPLE_CERT_PASSWORD    -R kostyk348/CapsuleCraft
gh secret set APPLE_ID               -R kostyk348/CapsuleCraft
gh secret set APPLE_TEAM_ID          -R kostyk348/CapsuleCraft
gh secret set APPLE_APP_PASSWORD     -R kostyk348/CapsuleCraft
gh secret set GPG_PRIVATE_KEY        -R kostyk348/CapsuleCraft < key.asc
gh secret set GPG_PASSPHRASE         -R kostyk348/CapsuleCraft
```

Без сертификатов установщики остаются функциональными — но с пометкой «неизвестный издатель»
(SmartScreen / Gatekeeper). Подпись контрольных сумм при этом есть всегда.
