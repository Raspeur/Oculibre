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
        versionCode = 9
        versionName = "0.3.5" // speaks protocol 0.3 (+ cameras' equivalent focal length)
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

    // Vendored sources (no Gradle dependency): ZXing's QR decoder, unchanged, with its licence.
    // See third_party/zxing/README.md.
    sourceSets["main"].java.srcDir("../third_party/zxing/src")
    // Its licence goes into the APK too (About › Open source licences), copied at build time.
    sourceSets["main"].assets.srcDir(layout.buildDirectory.dir("generated/licences"))

    testOptions {
        unitTests.all {
            // Shared protocol vectors (docs/protocol.typ, conformance tests).
            it.systemProperty("oculibre.vectors", rootProject.file("../tests/protocol/vectors").absolutePath)
        }
    }
}

// Intentionally no runtime dependencies: platform APIs only (small APK, F-Droid friendly, works on
// old devices). JUnit is only used by the unit tests on the build machine. Small vendored sources
// under a licence compatible with MIT are allowed, in third_party/ (#16).
// Licences of vendored code, read by the About screen (assets/licences/).
val copyLicences by tasks.registering(Copy::class) {
    from("../third_party/zxing/LICENSE") { rename { "zxing-LICENSE.txt" } }
    into(layout.buildDirectory.dir("generated/licences/licences"))
}
tasks.named("preBuild") { dependsOn(copyLicences) }

dependencies {
    testImplementation("junit:junit:4.13.2")
}
