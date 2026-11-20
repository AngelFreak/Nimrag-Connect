import java.io.ByteArrayOutputStream
import java.io.File
import javax.inject.Inject
import org.gradle.api.DefaultTask
import org.gradle.api.GradleException
import org.gradle.api.file.ConfigurableFileCollection
import org.gradle.api.file.DirectoryProperty
import org.gradle.api.file.RegularFileProperty
import org.gradle.api.provider.ListProperty
import org.gradle.api.provider.Property
import org.gradle.api.tasks.Input
import org.gradle.api.tasks.InputFile
import org.gradle.api.tasks.InputFiles
import org.gradle.api.tasks.Internal
import org.gradle.api.tasks.OutputDirectory
import org.gradle.api.tasks.PathSensitive
import org.gradle.api.tasks.PathSensitivity
import org.gradle.api.tasks.TaskAction
import org.gradle.process.ExecOperations

/**
 * Licence rule: no Android (or host-test) artifact may contain the locally
 * generated FIT profile (Garmin data, never redistributed). Every cargo
 * invocation in this build goes through [CargoTask.cargo], which forces
 * NIMRAG_FIT_PROFILE=none. After a build, [verifyNoFitProfile] reads cargo's
 * build-script output (core's `cfg(fit_profile)` must be off) and, for the
 * Android libraries, [verifyAndroidLibrary] greps the .so files themselves.
 */
const val FIT_PROFILE_ENV = "NIMRAG_FIT_PROFILE"

/**
 * The explicit opt-in of the owner's personal build (flavor `personal`, never published) that
 * core/build.rs and ffi/build.rs require before they accept a profile for an Android target.
 * Keep the name and value in sync with those two files.
 */
const val ALLOW_ANDROID_PROFILE_ENV = "NIMRAG_ALLOW_ANDROID_FIT_PROFILE"
const val ALLOW_ANDROID_PROFILE_VALUE = "personal-apk-never-published"

/** Emitted into any binary that contains the profile; see core/build.rs (keep in sync). */
const val FIT_PROFILE_MARKER = "NIMRAG-FIT-PROFILE-PRESENT-MARKER-7f3a9c"

fun verifyNoFitProfile(cargoTargetDir: File) {
    // Build-script output: `build/nimrag-core-<hash>/output` (older cargo) or
    // `build/nimrag-core/<hash>/run/stdout` (current cargo).
    val outputs = cargoTargetDir.walkTopDown()
        .filter { f ->
            f.isFile && (
                (f.name == "output" && f.parentFile.name.startsWith("nimrag-core-")) ||
                    (f.name == "stdout" && f.parentFile.name == "run" &&
                        f.parentFile.parentFile.parentFile.name == "nimrag-core")
                )
        }
        .toList()
    if (outputs.isEmpty()) {
        throw GradleException("No nimrag-core build-script output under $cargoTargetDir: cannot verify the FIT profile is off")
    }
    val on = outputs.filter { f -> f.readLines().any { it.contains("rustc-cfg=fit_profile") } }
    if (on.isNotEmpty()) {
        throw GradleException("core was built with cfg(fit_profile) ON (Garmin data): $on")
    }
}

private fun indexOf(haystack: ByteArray, needle: ByteArray, ignoreCase: Boolean): Boolean {
    if (needle.isEmpty() || haystack.size < needle.size) return false
    fun eq(a: Byte, b: Byte): Boolean {
        if (a == b) return true
        if (!ignoreCase) return false
        return a.toInt().toChar().lowercaseChar() == b.toInt().toChar().lowercaseChar()
    }
    for (i in 0..haystack.size - needle.size) {
        var j = 0
        while (j < needle.size && eq(haystack[i + j], needle[j])) j++
        if (j == needle.size) return true
    }
    return false
}

/** Checks on the finished Android .so files: no profile, no test fixtures, no build-machine paths. */
fun verifyAndroidLibrary(so: File, workspace: File, personal: Boolean = false) {
    val bytes = so.readBytes()
    fun has(text: String, ignoreCase: Boolean = false) = indexOf(bytes, text.toByteArray(Charsets.ISO_8859_1), ignoreCase)
    // Public: the profile must be absent. Personal: it must be present (else the build is not
    // what it says and the local web app would show nothing).
    if (!personal && has(FIT_PROFILE_MARKER)) {
        throw GradleException("$so contains the FIT profile marker (Garmin data)")
    }
    if (personal && !has(FIT_PROFILE_MARKER)) {
        throw GradleException("$so is a personal build but does not contain the FIT profile")
    }
    if (has("scriptedwatch", ignoreCase = true)) {
        throw GradleException("$so contains the test-fixtures ScriptedWatch")
    }
    if (has("/home/")) {
        throw GradleException("$so contains a build-machine path (/home/...): paths must be remapped")
    }
    if (has(workspace.absolutePath)) {
        throw GradleException("$so contains the workspace path ${workspace.absolutePath}")
    }
}

/** Runs cargo in the Rust workspace root with the FIT profile forced off. */
abstract class CargoTask : DefaultTask() {
    @get:Inject
    abstract val exec: ExecOperations

    /** Rust workspace root (contains Cargo.toml). */
    @get:Internal
    abstract val workspaceDir: DirectoryProperty

    /** Everything cargo reads: sources, manifests, lockfile, toolchain pin. */
    @get:InputFiles
    @get:PathSensitive(PathSensitivity.RELATIVE)
    abstract val rustSources: ConfigurableFileCollection

    /**
     * The owner's personal build: the generated FIT profile at [personalFitProfile] is compiled
     * in. Only the `personal` flavor sets this; every other cargo call forces the profile off.
     */
    @get:Input
    abstract val personalBuild: Property<Boolean>

    /** Absolute path of the generated profile (personal build only; kept outside version control). */
    @get:Internal
    abstract val personalFitProfile: Property<String>

    /** Cargo features for `-p nimrag-ffi` (the personal flavor adds `local-server`). */
    @get:Input
    abstract val ffiFeatures: ListProperty<String>

    init {
        personalBuild.convention(false)
        ffiFeatures.convention(emptyList())
    }

    protected open fun extraEnv(): Map<String, String> = emptyMap()

    protected fun cargo(vararg args: String) {
        exec.exec {
            workingDir = workspaceDir.get().asFile
            if (personalBuild.get()) {
                val profile = personalFitProfile.orNull?.let(::File)
                if (profile == null || !profile.isFile) {
                    throw GradleException(
                        "The personal flavor needs the generated FIT profile: set -Pnimrag.fitProfile=/path/to/profile_data.rs " +
                            "or NIMRAG_FIT_PROFILE_FILE (see tools/fit-profile/README.md). Got: $profile",
                    )
                }
                environment(FIT_PROFILE_ENV, profile.absolutePath)
                environment(ALLOW_ANDROID_PROFILE_ENV, ALLOW_ANDROID_PROFILE_VALUE)
            } else {
                environment(FIT_PROFILE_ENV, "none")
                // A leftover opt-in in the caller's environment must not reach a public build.
                environment(ALLOW_ANDROID_PROFILE_ENV, "")
            }
            environment(extraEnv())
            commandLine("cargo", *args)
        }
    }

    protected fun cargoOutput(vararg args: String): String {
        val out = ByteArrayOutputStream()
        exec.exec {
            workingDir = workspaceDir.get().asFile
            commandLine("cargo", *args)
            standardOutput = out
        }
        return out.toString().trim()
    }
}

/** `cargo ndk` for both ABIs into a jniLibs directory. */
abstract class CargoNdkTask : CargoTask() {
    @get:OutputDirectory
    abstract val jniLibsDir: DirectoryProperty

    /** cargo's target directory (for the FIT profile verification). */
    @get:Internal
    abstract val cargoTargetDir: DirectoryProperty

    @get:Input
    abstract val ndkVersion: Property<String>

    @get:Input
    abstract val cargoNdkVersion: Property<String>

    @get:Internal
    abstract val androidHome: Property<String>

    @get:Internal
    abstract val androidNdkHome: Property<String>

    private fun ndkDir(): File {
        val explicit = androidNdkHome.orNull?.takeIf { it.isNotBlank() }
        val dir = if (explicit != null) {
            File(explicit)
        } else {
            val sdk = androidHome.orNull?.takeIf { it.isNotBlank() }
                ?: throw GradleException("Set ANDROID_HOME (or ANDROID_NDK_HOME)")
            File(sdk, "ndk/${ndkVersion.get()}")
        }
        val props = File(dir, "source.properties")
        val revision = props.takeIf { it.isFile }?.readLines()
            ?.firstOrNull { it.startsWith("Pkg.Revision") }?.substringAfter("=")?.trim()
        if (revision != ndkVersion.get()) {
            throw GradleException("NDK at $dir is $revision, but this build pins ${ndkVersion.get()}")
        }
        return dir
    }

    override fun extraEnv(): Map<String, String> {
        // Reproducible output: no build-machine paths in the .so (panic locations, etc.).
        val cargoHome = System.getenv("CARGO_HOME") ?: "${System.getProperty("user.home")}/.cargo"
        val rustupHome = System.getenv("RUSTUP_HOME") ?: "${System.getProperty("user.home")}/.rustup"
        val flags = listOf(
            "--remap-path-prefix=$cargoHome=/cargo",
            "--remap-path-prefix=$rustupHome=/rustup",
            "--remap-path-prefix=${workspaceDir.get().asFile.absolutePath}=/src",
        ).joinToString(" ")
        val inherited = System.getenv("RUSTFLAGS")?.let { "$it " } ?: ""
        return mapOf("RUSTFLAGS" to inherited + flags, "ANDROID_NDK_HOME" to ndkDir().absolutePath)
    }

    @TaskAction
    fun build() {
        val actual = cargoOutput("ndk", "--version")
        if (actual != "cargo-ndk ${cargoNdkVersion.get()}") {
            throw GradleException("cargo-ndk is '$actual', but this build pins ${cargoNdkVersion.get()}")
        }
        val out = jniLibsDir.get().asFile
        out.deleteRecursively()
        val features = ffiFeatures.get()
        cargo(
            "ndk", "-t", "arm64-v8a", "-t", "x86_64", "-o", out.absolutePath,
            "build", "--release", "--locked", "-p", "nimrag-ffi",
            *(if (features.isEmpty()) emptyArray() else arrayOf("--features", features.joinToString(","))),
            "--target-dir", cargoTargetDir.get().asFile.absolutePath,
        )
        // Public: prove core's cfg(fit_profile) is off. Personal: the profile is meant to be in.
        if (!personalBuild.get()) verifyNoFitProfile(cargoTargetDir.get().asFile)
        for (abi in listOf("arm64-v8a", "x86_64")) {
            val so = File(out, "$abi/libnimrag_ffi.so")
            if (!so.isFile) throw GradleException("cargo ndk did not produce $abi/libnimrag_ffi.so")
            verifyAndroidLibrary(so, workspaceDir.get().asFile.canonicalFile, personal = personalBuild.get())
        }
    }
}

/**
 * Host (x86_64-linux-gnu) build of libnimrag_ffi.so WITH the `test-fixtures`
 * feature, for the JVM test only. Its own target dir, so it can never be
 * confused with the release library that goes into the APK. Only the .so is
 * the task's output (a small directory), so the test depends on its content.
 */
abstract class CargoHostTestLibTask : CargoTask() {
    @get:Internal
    abstract val cargoTargetDir: DirectoryProperty

    @get:OutputDirectory
    abstract val libDir: DirectoryProperty

    @TaskAction
    fun build() {
        val dir = cargoTargetDir.get().asFile
        cargo(
            "build", "--release", "--locked", "-p", "nimrag-ffi", "--features", "test-fixtures",
            "--target-dir", dir.absolutePath,
        )
        verifyNoFitProfile(dir)
        val out = libDir.get().asFile
        out.deleteRecursively()
        out.mkdirs()
        File(dir, "release/libnimrag_ffi.so").copyTo(File(out, "libnimrag_ffi.so"))
    }
}

/** Generates the Kotlin bindings from a built library (UniFFI library mode). */
abstract class UniffiKotlinTask : CargoTask() {
    @get:InputFile
    @get:PathSensitive(PathSensitivity.NONE)
    abstract val library: RegularFileProperty

    @get:Internal
    abstract val bindgenTargetDir: DirectoryProperty

    @get:OutputDirectory
    abstract val outputDir: DirectoryProperty

    @TaskAction
    fun generate() {
        val out = outputDir.get().asFile
        out.deleteRecursively()
        cargo(
            "run", "--quiet", "--locked", "-p", "nimrag-ffi", "--features", "bindgen", "--bin", "uniffi-bindgen",
            "--target-dir", bindgenTargetDir.get().asFile.absolutePath,
            "--", "generate", "--library", library.get().asFile.absolutePath,
            "--language", "kotlin", "--out-dir", out.absolutePath, "--no-format",
        )
        verifyNoFitProfile(bindgenTargetDir.get().asFile)
    }
}
