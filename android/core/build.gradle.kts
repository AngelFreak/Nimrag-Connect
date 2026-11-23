plugins {
    alias(libs.plugins.android.library)
}

val workspace = rootProject.layout.projectDirectory.dir("..")

/**
 * Two flavors (dimension `distribution`):
 * Both flavors compile in the in-process server (cargo feature `local-server`, with the web app
 * from web/dist embedded), so the phone derives and shows the newest data without the server.
 *  - `public`: the build that goes to GitHub releases. No Garmin FIT profile (its licence forbids
 *    shipping it in AGPL software): the cargo build forces NIMRAG_FIT_PROFILE=none, which compiles
 *    in Nimrag's own clean-room field map instead. Verified after every build.
 *  - `personal`: the owner's own build, never published. The same, plus the generated Garmin FIT
 *    profile (more names and the workout/weather writers). Needs
 *    -Pnimrag.fitProfile=/path/to/profile_data.rs (or NIMRAG_FIT_PROFILE_FILE).
 */
val fitProfileFile: Provider<String> = providers.gradleProperty("nimrag.fitProfile")
    .orElse(providers.environmentVariable("NIMRAG_FIT_PROFILE_FILE"))

fun rustInputs() = files(
    workspace.file("Cargo.toml"),
    workspace.file("Cargo.lock"),
    workspace.file("rust-toolchain.toml"),
    workspace.dir("core").asFileTree.matching {
        include("Cargo.toml", "build.rs", "src/**")
        exclude("src/fit/generated/**")
    },
    workspace.dir("ffi").asFileTree.matching { include("Cargo.toml", "build.rs", "src/**") },
).also {
    it.from(
        workspace.dir("server").asFileTree.matching {
            include("Cargo.toml", "build.rs", "src/**", "migrations/**", ".sqlx/**")
        },
        // Embedded into the library by rust-embed at compile time (`npm run build` first).
        workspace.dir("web/dist").asFileTree,
    )
}

fun cap(s: String) = s.replaceFirstChar { it.uppercase() }

fun registerCargoNdk(flavor: String, personal: Boolean) = tasks.register<CargoNdkTask>("cargoNdk${cap(flavor)}") {
    group = "rust"
    description = if (personal) {
        "Builds libnimrag_ffi.so (arm64-v8a, x86_64) WITH Garmin's FIT profile and the local server: personal flavor, never published"
    } else {
        "Builds libnimrag_ffi.so (arm64-v8a, x86_64) with the local server and NIMRAG_FIT_PROFILE=none (Nimrag's own field map, no Garmin profile)"
    }
    workspaceDir = workspace
    rustSources.from(rustInputs())
    jniLibsDir = layout.buildDirectory.dir("generated/jniLibs-$flavor")
    cargoTargetDir = layout.buildDirectory.dir("cargo-ndk-$flavor")
    ndkVersion = libs.versions.ndk
    cargoNdkVersion = libs.versions.cargo.ndk
    androidHome = providers.environmentVariable("ANDROID_HOME")
    androidNdkHome = providers.environmentVariable("ANDROID_NDK_HOME")
    ffiFeatures = listOf("local-server")
    doFirst {
        if (!workspace.file("web/dist/index.html").asFile.isFile) {
            throw GradleException("web/dist is missing: run `npm run build` in web/ first (the build embeds the web app)")
        }
    }
    if (personal) {
        personalBuild = true
        personalFitProfile = fitProfileFile
    }
}

fun registerUniffi(flavor: String, from: TaskProvider<CargoNdkTask>) =
    tasks.register<UniffiKotlinTask>("uniffiKotlin${cap(flavor)}") {
        group = "rust"
        description = "Generates the Kotlin bindings of the $flavor flavor from its x86_64 library"
        workspaceDir = workspace
        rustSources.from(rustInputs())
        library = from.flatMap { t -> t.jniLibsDir.file("x86_64/libnimrag_ffi.so") }
        bindgenTargetDir = layout.buildDirectory.dir("cargo-bindgen")
        outputDir = layout.buildDirectory.dir("generated/uniffi-$flavor")
    }

val cargoNdkPublic = registerCargoNdk("public", personal = false)
val cargoNdkPersonal = registerCargoNdk("personal", personal = true)
val uniffiPublic = registerUniffi("public", cargoNdkPublic)
val uniffiPersonal = registerUniffi("personal", cargoNdkPersonal)

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
