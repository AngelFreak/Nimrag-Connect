plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.compose)
}

android {
    namespace = "dev.nimrag.app"
    compileSdk {
        version = release(37) { minorApiLevel = 2 }
    }
    ndkVersion = libs.versions.ndk.get()
    defaultConfig {
        applicationId = "dev.nimrag.app"
        minSdk = 31
        targetSdk = 37
        versionCode = 25
        versionName = "0.8.5"
        // Only the ABIs the Rust library is built for (this also drops JNA's other ABIs).
        ndk { abiFilters += listOf("arm64-v8a", "x86_64") }
    }
    buildTypes {
        release {
            isMinifyEnabled = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
        }
    }
    buildFeatures {
        compose = true
        buildConfig = true
    }
    // Both flavors have the local server (the phone's own copy of the web app). `public` is the
    // published build, with Nimrag's own FIT field map and no Garmin profile; `personal` is the
    // owner's own build with Garmin's FIT profile too, never published. FIT_PROFILE says which
    // library the build must have (MainActivity checks it). See core/build.gradle.kts.
    flavorDimensions += "distribution"
    productFlavors {
        create("public") {
            dimension = "distribution"
            buildConfigField("boolean", "LOCAL_SERVER", "true")
            buildConfigField("boolean", "FIT_PROFILE", "false")
        }
        create("personal") {
            dimension = "distribution"
            versionNameSuffix = "-personal"
            buildConfigField("boolean", "LOCAL_SERVER", "true")
            buildConfigField("boolean", "FIT_PROFILE", "true")
        }
    }
    testOptions {
        // JUnit Jupiter (same catalogue entries as :hosttest). The engine's tests only touch
        // pure Kotlin/JDK classes; Android framework calls would fail loudly.
        unitTests.all { it.useJUnitPlatform() }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}

dependencies {
    implementation(project(":core"))
    implementation(platform(libs.compose.bom))
    implementation(libs.compose.ui)
    implementation(libs.compose.ui.tooling.preview)
    implementation(libs.compose.material3)
    implementation(libs.compose.material.icons.core)
    implementation(libs.activity.compose)
    implementation(libs.lifecycle.runtime.compose)
    implementation(libs.kotlinx.coroutines.android)
    implementation(libs.webkit)
    debugImplementation(libs.compose.ui.tooling)
    testImplementation(libs.junit.jupiter)
    testRuntimeOnly(libs.junit.platform.launcher)
}
