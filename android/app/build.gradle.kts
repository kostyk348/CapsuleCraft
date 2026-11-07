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

    // Release signing comes from the environment (CI secrets). Without it a
    // build falls back to the auto-generated debug key so `gradle assemble`
    // never blocks locally.
    signingConfigs {
        create("release") {
            val ks = System.getenv("ANDROID_KEYSTORE_FILE") ?: ""
            if (ks.isNotEmpty()) {
                storeFile = file(ks)
                storePassword = System.getenv("ANDROID_KEYSTORE_PASSWORD")
                keyAlias = System.getenv("ANDROID_KEY_ALIAS")
                keyPassword = System.getenv("ANDROID_KEY_PASSWORD")
            }
        }
    }
    buildTypes {
        release {
            isMinifyEnabled = false
            signingConfig = if (System.getenv("ANDROID_KEYSTORE_FILE")?.isNotEmpty() == true)
                signingConfigs.getByName("release") else signingConfigs.getByName("debug")
        }
    }
}
