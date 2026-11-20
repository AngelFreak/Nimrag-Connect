plugins {
    alias(libs.plugins.android.library)
}

val workspace = rootProject.layout.projectDirectory.dir("..")

/**
 * Two flavors (dimension `distribution`):
 *  - `public`: the build that goes to GitHub releases. No FIT profile (Garmin's licence forbids
 *    shipping it in AGPL software), no local server. Verified after every build.
 *  - `personal`: the owner's own build, never published. The generated FIT profile and the
 *    in-process server (cargo feature `local-server`, with the web app embedded) are compiled in,
 *    so the phone derives and shows the newest data without the server. Needs
 *    -Pnimrag.fitProfile=/path/to/profile_data.rs (or NIMRAG_FIT_PROFILE_FILE) and web/dist.
 */
val fitProfileFile: Provider<String> = providers.gradleProperty("nimrag.fitProfile")
    .orElse(providers.environmentVariable("NIMRAG_FIT_PROFILE_FILE"))

fun rustInputs(withServer: Boolean) = files(
    workspace.file("Cargo.toml"),
    workspace.file("Cargo.lock"),
    workspace.file("rust-toolchain.toml"),
    workspace.dir("core").asFileTree.matching {
        include("Cargo.toml", "build.rs", "src/**")
        exclude("src/fit/generated/**")
    },
    workspace.dir("ffi").asFileTree.matching { include("Cargo.toml", "build.rs", "src/**") },
).also {
    if (withServer) {
        it.from(
            workspace.dir("server").asFileTree.matching {
                include("Cargo.toml", "build.rs", "src/**", "migrations/**", ".sqlx/**")
            },
            // Embedded into the library by rust-embed at compile time (`npm run build` first).
            workspace.dir("web/dist").asFileTree,
        )
    }
}

fun cap(s: String) = s.replaceFirstChar { it.uppercase() }

fun registerCargoNdk(flavor: String, personal: Boolean) = tasks.register<CargoNdkTask>("cargoNdk${cap(flavor)}") {
    group = "rust"
    description = if (personal) {
        "Builds libnimrag_ffi.so (arm64-v8a, x86_64) WITH the FIT profile and the local server: personal flavor, never published"
    } else {
        "Builds libnimrag_ffi.so for arm64-v8a and x86_64 with NIMRAG_FIT_PROFILE=none"
    }
    workspaceDir = workspace
    rustSources.from(rustInputs(withServer = personal))
    jniLibsDir = layout.buildDirectory.dir("generated/jniLibs-$flavor")
    cargoTargetDir = layout.buildDirectory.dir("cargo-ndk-$flavor")
    ndkVersion = libs.versions.ndk
    cargoNdkVersion = libs.versions.cargo.ndk
    androidHome = providers.environmentVariable("ANDROID_HOME")
    androidNdkHome = providers.environmentVariable("ANDROID_NDK_HOME")
    if (personal) {
        personalBuild = true
        personalFitProfile = fitProfileFile
        ffiFeatures = listOf("local-server")
        doFirst {
            if (!workspace.file("web/dist/index.html").asFile.isFile) {
                throw GradleException("web/dist is missing: run `npm run build` in web/ first (the personal build embeds the web app)")
            }
        }
    }
}

fun registerUniffi(flavor: String, from: TaskProvider<CargoNdkTask>, personal: Boolean) =
    tasks.register<UniffiKotlinTask>("uniffiKotlin${cap(flavor)}") {
        group = "rust"
        description = "Generates the Kotlin bindings of the $flavor flavor from its x86_64 library"
        workspaceDir = workspace
        rustSources.from(rustInputs(withServer = personal))
        library = from.flatMap { t -> t.jniLibsDir.file("x86_64/libnimrag_ffi.so") }
        bindgenTargetDir = layout.buildDirectory.dir("cargo-bindgen")
        outputDir = layout.buildDirectory.dir("generated/uniffi-$flavor")
    }

val cargoNdkPublic = registerCargoNdk("public", personal = false)
val cargoNdkPersonal = registerCargoNdk("personal", personal = true)
val uniffiPublic = registerUniffi("public", cargoNdkPublic, personal = false)
val uniffiPersonal = registerUniffi("personal", cargoNdkPersonal, personal = true)

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
    flavorDimensions += "distribution"
    productFlavors {
        create("public") { dimension = "distribution" }
        create("personal") { dimension = "distribution" }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}

androidComponents.onVariants { variant ->
    val personal = variant.flavorName == "personal"
    val cargo = if (personal) cargoNdkPersonal else cargoNdkPublic
    val uniffi = if (personal) uniffiPersonal else uniffiPublic
    variant.sources.jniLibs?.addGeneratedSourceDirectory(cargo, CargoNdkTask::jniLibsDir)
    variant.sources.kotlin?.addGeneratedSourceDirectory(uniffi, UniffiKotlinTask::outputDir)
}

dependencies {
    // UniFFI's generated Kotlin loads the library through JNA (the Android artifact).
    api(variantOf(libs.jna) { artifactType("aar") })
}
