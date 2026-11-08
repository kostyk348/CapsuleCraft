# Android

Статус: **скаффолд, начиная с v2.9.0**. Тот же нативный egui-бинарь собирается
как `NativeActivity` под Android (arm64-v8a) — без WebView, тот же код, то же
хранилище `.eml`.

## Что уже есть

- `native/src/main.rs::android_main` — точка входа: логирование через
  `android_logger`, внутренняя папка данных приложения передаётся в
  `vault::set_android_data_dir`, хранилище живёт в
  `<internal>/capsulecraft/vault`.
- `native/Cargo.toml` — target-зависимости `aarch64`: eframe с фичей
  `android-game-activity`, `android-activity 0.6` (та же мажорная версия,
  что у winit, чтобы тип `AndroidApp` совпал), `android_logger`; тот же
  `src/main.rs` объявлен ещё и целью `[lib]` с `crate-type = ["cdylib"]` —
  NativeActivity грузит приложение как `libcapsuleos_native.so`, а не как
  бинарь (у bin-цели под Android нет `main` — E0601).
- `android/` — минимальный gradle-проект: `NativeActivity` с
  `android.app.lib_name = capsuleos_native`, `hasCode=false` — никакого
  собственного Java/Kotlin-кода.
- `.github/workflows/android.yml` — CI собирает `libcapsuleos_native.so`
  через `cargo ndk … --lib` и упаковывает debug-APK (артефакт джобы).

## Сборка локально

```bash
rustup target add aarch64-linux-android
cargo install cargo-ndk --locked
cd native
cargo ndk -t arm64-v8a -o ../android/app/src/main/jniLibs build --release --lib
cd ../android
gradle assembleDebug
# → app/build/outputs/apk/debug/app-debug.apk
```

## Синхронизация между устройствами

Движок `native/src/sync.rs` — двустороннее слияние «хранилище ↔ папка»:
last-writer-wins по mtime с допуском 2 с на расхождение часов, файлы,
есть только с одной стороны, копируются на недостающую. В UI — кнопка
«Синхронизировать» в подвале сайдбара (Shift — выбрать папку заново).

На телефоне папка синхронизации на SD-карте пока недоступна (нет
запросов разрешений) — следующий шаг: SAF-пикер каталога, тогда папку
можно будет указать в Nextcloud/Dropbox на устройстве и синхронизироваться
напрямую с телефона. Сегодня связка такая: телефон экспортирует/импортирует
страницы через «Поделиться», настольные устройства объединяются папкой.

## Дорожная карта

1. SAF-пикер каталога + разрешение на запись в выбранную папку.
2. Автосинхронизация при открытии приложения и по таймеру.
3. Электронная подпись объединения (hash-манифест) для защиты от
    конфликтов правок одной страницы на двух устройствах — сейчас
    конфликт молча решается в пользу более свежей правки.
4. Иконка и сплэш (сейчас системные), выпуск arm32/x86_64 по запросу.

## v2.12.0: NativeActivity backend and accesskit

The shell runs as a NativeActivity (`android.app.NativeActivity`, no Java
code). The Rust lib builds against android-activity's `native-activity`
backend; eframe is declared per-target and the desktop-only `accesskit`
feature is NOT enabled here — eframe 0.37 fails to compile with accesskit +
android-native-activity together. Before v2.12.0 the crate asked for
`android-game-activity` while the manifest shipped NativeActivity: the
mismatch produced a `.so` without `ANativeActivity_onCreate`, and the app
crashed at launch.
