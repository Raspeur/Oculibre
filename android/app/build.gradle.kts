plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "org.oculibre"
    compileSdk = 35

    defaultConfig {
        applicationId = "org.oculibre"
        minSdk = 21 // Android 5.0 — the whole point of the project
        targetSdk = 35
        versionCode = 1
        versionName = "0.0.1"
    }

    buildTypes {
        release {
            isMinifyEnabled = false
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    kotlinOptions {
        jvmTarget = "17"
    }
}

// Intentionally no dependencies: platform APIs only (small APK, F-Droid friendly, works on old devices).
