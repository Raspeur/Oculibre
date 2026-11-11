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
        versionCode = 3
        versionName = "0.2.0" // speaks protocol 0.2
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

    testOptions {
        unitTests.all {
            // Shared protocol vectors (docs/protocol.typ, conformance tests).
            it.systemProperty("oculibre.vectors", rootProject.file("../tests/protocol/vectors").absolutePath)
        }
    }
}

// Intentionally no runtime dependencies: platform APIs only (small APK, F-Droid friendly, works on
// old devices). JUnit is only used by the unit tests on the build machine.
dependencies {
    testImplementation("junit:junit:4.13.2")
}
