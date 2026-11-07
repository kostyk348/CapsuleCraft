plugins {
    id("com.android.application")
}

android {
    namespace = "com.capsulecraft"
    compileSdk = 35
    defaultConfig {
        applicationId = "com.capsulecraft"
        minSdk = 24
        targetSdk = 35
        versionCode = 1
        versionName = "2.9.1"
        ndk {
            // The Rust lib is built for arm64 by CI (cargo ndk); x86_64 can
            // be added for emulator runs.
            abiFilters += listOf("arm64-v8a")
        }
    }
}
