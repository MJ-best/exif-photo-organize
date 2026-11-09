// Google's mirror of Maven Central first: Maven Central limits large downloads (429) from shared
// addresses such as CI runners.
val centralMirror = "https://maven-central.storage-download.googleapis.com/maven2/"

pluginManagement {
    repositories {
        google()
        maven("https://maven-central.storage-download.googleapis.com/maven2/")
        mavenCentral()
        gradlePluginPortal()
    }
}

dependencyResolutionManagement {
    repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS)
    repositories {
        google()
        maven(centralMirror)
        mavenCentral()
    }
}

rootProject.name = "PhotoOrganize"
include(":app")
