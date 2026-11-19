plugins {
    alias(libs.plugins.kotlin.jvm)
}

// Pure-JVM tests of the real UniFFI bindings against the x86_64-linux-gnu build of
// libnimrag_ffi.so (with the `test-fixtures` feature; never part of the APK).
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

val cargoHost = tasks.register<CargoHostTestLibTask>("cargoHostLib") {
    group = "rust"
    description = "Builds the host libnimrag_ffi.so with test-fixtures and NIMRAG_FIT_PROFILE=none"
    workspaceDir = workspace
    rustSources.from(rustInputs)
    cargoTargetDir = layout.buildDirectory.dir("cargo-host")
    libDir = layout.buildDirectory.dir("host-lib")
}

val uniffiKotlin = tasks.register<UniffiKotlinTask>("uniffiKotlin") {
    group = "rust"
    description = "Generates the Kotlin bindings (including the test fixtures) from the host library"
    workspaceDir = workspace
    rustSources.from(rustInputs)
    library = cargoHost.flatMap { t -> t.libDir.file("libnimrag_ffi.so") }
    bindgenTargetDir = layout.buildDirectory.dir("cargo-bindgen")
    outputDir = layout.buildDirectory.dir("generated/uniffi")
}

sourceSets.named("test") {
    kotlin.srcDir(uniffiKotlin.flatMap { it.outputDir })
}

dependencies {
    testImplementation(libs.jna)
    testImplementation(libs.junit.jupiter)
    testRuntimeOnly(libs.junit.platform.launcher)
}

tasks.test {
    useJUnitPlatform()
    jvmArgs("--enable-native-access=ALL-UNNAMED")
    // The library is a task input, so a rebuilt .so re-runs the tests.
    val libDir = cargoHost.flatMap { it.libDir }
    inputs.dir(libDir).withPathSensitivity(PathSensitivity.NONE).withPropertyName("nativeLib")
    jvmArgumentProviders.add(CommandLineArgumentProvider { listOf("-Djna.library.path=${libDir.get().asFile.absolutePath}") })
    testLogging {
        events("passed", "failed", "skipped")
        showStandardStreams = true
    }
}
