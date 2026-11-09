import org.jetbrains.kotlin.gradle.dsl.JvmTarget
import java.util.Properties

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
    id("io.github.takahirom.roborazzi")
}

// The Rust workspace (the engine) is the repository root.
val repoRoot: File = rootProject.projectDir.parentFile
val cargo = (findProperty("cargo") as String?) ?: "cargo"
// arm64-v8a: phones and tablets; x86_64: emulators and Chromebooks.
val rustAbis = ((findProperty("rustAbis") as String?) ?: "arm64-v8a,x86_64").split(",").map { it.trim() }

// One version for every platform: the workspace's (Cargo.toml).
val appVersion: String = Regex("""(?m)^version = "([^"]+)"""")
    .find(repoRoot.resolve("Cargo.toml").readText())!!.groupValues[1]

fun versionCodeOf(v: String): Int {
    val (major, minor, patch) = v.split(".").map { it.takeWhile(Char::isDigit).toInt() }
    // 0.5.0 -> 5000; the last digit is left for re-uploads of the same version (-PversionCode).
    return major * 100_000 + minor * 1_000 + patch * 10
}

// Release signing (upload key) from keystore.properties or the environment (CI).
val signing = Properties().apply {
    val f = rootProject.file("keystore.properties")
    if (f.exists()) f.inputStream().use { load(it) }
    System.getenv("ANDROID_KEYSTORE_FILE")?.let { setProperty("storeFile", it) }
    System.getenv("ANDROID_KEYSTORE_PASSWORD")?.let { setProperty("storePassword", it) }
    System.getenv("ANDROID_KEY_ALIAS")?.let { setProperty("keyAlias", it) }
    System.getenv("ANDROID_KEY_PASSWORD")?.let { setProperty("keyPassword", it) }
}
val canSign = listOf("storeFile", "storePassword", "keyAlias", "keyPassword").all { !signing.getProperty(it).isNullOrBlank() }

android {
    namespace = "io.github.mj_best.photoorganize"
    compileSdk = 36
    ndkVersion = "28.2.13676358"

    defaultConfig {
        applicationId = "io.github.mj_best.photoorganize"
        minSdk = 26
        targetSdk = 36
        versionCode = (findProperty("versionCode") as String?)?.toInt() ?: versionCodeOf(appVersion)
        versionName = appVersion
        ndk { abiFilters += rustAbis }
    }

    signingConfigs {
        if (canSign) {
            create("upload") {
                storeFile = file(signing.getProperty("storeFile"))
                storePassword = signing.getProperty("storePassword")
                keyAlias = signing.getProperty("keyAlias")
                keyPassword = signing.getProperty("keyPassword")
            }
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
            if (canSign) signingConfig = signingConfigs.getByName("upload")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    buildFeatures {
        compose = true
        buildConfig = true
    }

    sourceSets["main"].java.srcDir(layout.buildDirectory.dir("generated/uniffi"))
    sourceSets["main"].jniLibs.srcDir(layout.buildDirectory.dir("rustJniLibs"))

    testOptions {
        unitTests {
            isIncludeAndroidResources = true
            all { it.maxHeapSize = "3g" }
        }
    }

    packaging {
        // Stored uncompressed and 16 KB aligned in the APK, loaded straight from it.
        jniLibs { useLegacyPackaging = false }
    }
}

kotlin {
    compilerOptions { jvmTarget.set(JvmTarget.JVM_17) }
}

// ---------------------------------------------------------------------------------------------
// The engine: Rust, built with cargo-ndk for the device and for this computer (Kotlin bindings,
// unit tests).

val rustJniLibs = layout.buildDirectory.dir("rustJniLibs")
val hostLib: File = repoRoot.resolve("target/debug/" + System.mapLibraryName("photoorg_android"))

val buildRust by tasks.registering(Exec::class) {
    group = "rust"
    description = "Builds the engine for Android (cargo ndk)."
    workingDir = repoRoot
    val out = rustJniLibs.get().asFile
    commandLine(
        listOf(cargo, "ndk") + rustAbis.flatMap { listOf("-t", it) } +
            listOf("-P", "26", "-o", out.path, "build", "--release", "-p", "photoorg-android"),
    )
    environment("ANDROID_NDK_HOME", android.ndkDirectory.path)
    inputs.dir(repoRoot.resolve("crates"))
    inputs.file(repoRoot.resolve("Cargo.lock"))
    outputs.dir(out)
}

val buildHostRust by tasks.registering(Exec::class) {
    group = "rust"
    description = "Builds the engine for this computer (bindings, unit tests)."
    workingDir = repoRoot
    commandLine(cargo, "build", "-p", "photoorg-android")
    inputs.dir(repoRoot.resolve("crates"))
    inputs.file(repoRoot.resolve("Cargo.lock"))
    outputs.file(hostLib)
}

val generateBindings by tasks.registering(Exec::class) {
    group = "rust"
    description = "Generates the Kotlin bindings of the engine (UniFFI)."
    dependsOn(buildHostRust)
    workingDir = repoRoot
    val out = layout.buildDirectory.dir("generated/uniffi").get().asFile
    commandLine(
        cargo, "run", "-q", "-p", "photoorg-android", "--features", "bindgen", "--bin", "uniffi-bindgen", "--",
        "generate", "--library", hostLib.path, "--language", "kotlin", "--no-format", "--out-dir", out.path,
    )
    inputs.file(hostLib)
    outputs.dir(out)
}

tasks.named("preBuild") { dependsOn(generateBindings) }
afterEvaluate {
    // Only packaging needs the device libraries; unit tests use the host one.
    tasks.matching { it.name.startsWith("merge") && it.name.endsWith("JniLibFolders") }
        .configureEach { dependsOn(buildRust) }
}

// Robolectric's Android (the version Robolectric 4.16 runs SDK 36 on), fetched by Gradle rather
// than by Robolectric at test time.
val robolectricSdk: Configuration by configurations.creating
val robolectricJars by tasks.registering(Copy::class) {
    from(robolectricSdk)
    into(layout.buildDirectory.dir("robolectric"))
}

tasks.withType<Test>().configureEach {
    // Robolectric 4.16 runs SDK 36 only on Java 21+; otherwise every test class fails with an
    // UnsupportedOperationException from DefaultSdkProvider.
    doFirst {
        check(JavaVersion.current() >= JavaVersion.VERSION_21) {
            "Android unit tests need Java 21 or newer (Robolectric, SDK 36); this Gradle runs on Java ${JavaVersion.current()}."
        }
    }
    dependsOn(buildHostRust, robolectricJars)
    systemProperty("jna.library.path", hostLib.parentFile.path)
    systemProperty("robolectric.offline", "true")
    systemProperty("robolectric.dependency.dir", layout.buildDirectory.dir("robolectric").get().asFile.path)
    systemProperty("robolectric.pixelCopyRenderMode", "hardware")
    // Screenshots of the screens (build/outputs/roborazzi); -Pdemo=<library folder> uses a real library.
    systemProperty("roborazzi.test.record", "true")
    // Korean file names (작품 folders) on machines whose default locale is not UTF-8.
    environment("LC_ALL", "C.UTF-8")
    (findProperty("demo") as String?)?.let { systemProperty("photoorg.demo", it) }
}

dependencies {
    val composeBom = platform("androidx.compose:compose-bom:2025.12.01")
    implementation(composeBom)
    implementation("androidx.core:core-ktx:1.17.0")
    implementation("androidx.activity:activity-compose:1.11.0")
    implementation("androidx.lifecycle:lifecycle-viewmodel-compose:2.9.4")
    implementation("androidx.lifecycle:lifecycle-runtime-compose:2.9.4")
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.foundation:foundation")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.material:material-icons-extended:1.7.8")
    implementation("androidx.compose.ui:ui-tooling-preview")
    debugImplementation("androidx.compose.ui:ui-tooling")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.10.2")
    // UniFFI's Kotlin bindings call the engine through JNA.
    implementation("net.java.dev.jna:jna:5.17.0@aar")

    testImplementation("junit:junit:4.13.2")
    testImplementation("org.robolectric:robolectric:4.16")
    // The desktop JNA (with its native part for this computer) for unit tests.
    testImplementation("net.java.dev.jna:jna:5.17.0")
    testImplementation(composeBom)
    testImplementation("androidx.compose.ui:ui-test-junit4")
    debugImplementation("androidx.compose.ui:ui-test-manifest")
    testImplementation("io.github.takahirom.roborazzi:roborazzi:1.52.0")
    testImplementation("io.github.takahirom.roborazzi:roborazzi-compose:1.52.0")
    testImplementation("io.github.takahirom.roborazzi:roborazzi-junit-rule:1.52.0")
    robolectricSdk("org.robolectric:android-all-instrumented:16-robolectric-13921718-i7")
}
