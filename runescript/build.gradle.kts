plugins {
    // Kotlin 2.2.x to match the RustRover 2025.3 platform's bundled Kotlin
    // metadata (it ships Kotlin 2.2.0; 2.0.x can't read its .kotlin_module files).
    kotlin("jvm") version "2.2.0" apply false
    id("org.jetbrains.intellij.platform") version "2.1.0" apply false
}

allprojects {
    group = "com.os.runescript"
    version = "0.4.9"

    repositories {
        mavenCentral()
    }
}

subprojects {
    apply(plugin = "org.jetbrains.kotlin.jvm")

    // The JVM language level the compiler + plugin target. 17 is the floor
    // for current IntelliJ platform releases, so the shared frontend stays
    // loadable inside the IDE.
    extensions.configure<org.jetbrains.kotlin.gradle.dsl.KotlinJvmProjectExtension> {
        jvmToolchain(17)
    }

    dependencies {
        "testImplementation"(kotlin("test"))
    }

    tasks.withType<Test> {
        useJUnitPlatform()
    }
}
