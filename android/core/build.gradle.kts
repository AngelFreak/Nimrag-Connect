plugins {
    alias(libs.plugins.android.library)
}

val workspace = rootProject.layout.projectDirectory.dir("..")
val rustInputs = files(
    workspace.file("Cargo.toml"),
    workspace.file("Cargo.lock"),
    workspace.file("rust-toolchain.toml"),
    workspace.dir("core").asFileTree.matching {
        include("Cargo.toml", "build.rs", "src/**")
        exclude("src/fit/generated/**")
    },
    workspace.dir("ffi").asFileTree.matching { include("Cargo.toml", "build.rs", "src/**") },
)

val cargoNdk = tasks.register<CargoNdkTask>("cargoNdk") {
    group = "rust"
    description = "Builds libnimrag_ffi.so for arm64-v8a and x86_64 with NIMRAG_FIT_PROFILE=none"
    workspaceDir = workspace
    rustSources.from(rustInputs)
    jniLibsDir = layout.buildDirectory.dir("generated/jniLibs")
    cargoTargetDir = layout.buildDirectory.dir("cargo-ndk")
    ndkVersion = libs.versions.ndk
    cargoNdkVersion = libs.versions.cargo.ndk
    androidHome = providers.environmentVariable("ANDROID_HOME")
    androidNdkHome = providers.environmentVariable("ANDROID_NDK_HOME")
}

val uniffiKotlin = tasks.register<UniffiKotlinTask>("uniffiKotlin") {
    group = "rust"
    description = "Generates the Kotlin bindings from the x86_64 library"
    workspaceDir = workspace
    rustSources.from(rustInputs)
    library = cargoNdk.flatMap { t -> t.jniLibsDir.file("x86_64/libnimrag_ffi.so") }
    bindgenTargetDir = layout.buildDirectory.dir("cargo-bindgen")
    outputDir = layout.buildDirectory.dir("generated/uniffi")
}

android {
    namespace = "dev.nimrag.core"
    compileSdk {
        version = release(37) { minorApiLevel = 2 }
    }
    ndkVersion = libs.versions.ndk.get()
    defaultConfig {
        minSdk = 31
        ndk { abiFilters += listOf("arm64-v8a", "x86_64") }
        consumerProguardFiles("consumer-rules.pro")
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}

androidComponents.onVariants { variant ->
    variant.sources.jniLibs?.addGeneratedSourceDirectory(cargoNdk, CargoNdkTask::jniLibsDir)
    variant.sources.kotlin?.addGeneratedSourceDirectory(uniffiKotlin, UniffiKotlinTask::outputDir)
}

dependencies {
    // UniFFI's generated Kotlin loads the library through JNA (the Android artifact).
    api(variantOf(libs.jna) { artifactType("aar") })
}
