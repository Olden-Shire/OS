import org.jetbrains.intellij.platform.gradle.IntelliJPlatformType
import org.jetbrains.intellij.platform.gradle.TestFrameworkType

plugins {
    id("org.jetbrains.intellij.platform")
}

repositories {
    mavenCentral()
    intellijPlatform {
        defaultRepositories()
    }
}

dependencies {
    // The shared compiler frontend + compiler/decompiler back the plugin's
    // lexer, references, and Compile/Decompile actions.
    implementation(project(":frontend"))
    implementation(project(":compiler"))

    // The IntelliJ platform test runner injects a JUnit4-based session
    // listener, so JUnit4 must be on the test classpath even though our tests
    // are written with kotlin.test/JUnit5. The vintage engine lets JUnit
    // Platform discover the JUnit3/4 BasePlatformTestCase tests too.
    testImplementation("junit:junit:4.13.2")
    testRuntimeOnly("org.junit.vintage:junit-vintage-engine:5.10.2")

    intellijPlatform {
        // Target the user's actual IDE so the plugin is built against the same
        // platform/plugin-descriptor model it runs on (RustRover 2025.3 / 253).
        rustRover("2025.3")
        testFramework(TestFrameworkType.Platform)
    }
}

intellijPlatform {
    instrumentCode = false
    pluginConfiguration {
        id = "com.os1.runescript"
        name = "RuneScript (OS1)"
        version = project.version.toString()
        ideaVersion {
            sinceBuild = "253"
            // Open-ended: clear until-build so the plugin loads in newer IDEs
            // (e.g. RustRover 2025.3 / build 253). The plugin only uses stable
            // platform APIs, so it's forward-compatible.
            untilBuild = provider { null }
        }
        vendor {
            name = "OS1"
        }
    }
    buildSearchableOptions = false
}

// The 2025.3 platform runs on JDK 21, so this module targets 21 (overriding the
// repo-wide 17 default). The frontend/compiler libs stay 17 and run fine on 21.
kotlin {
    jvmToolchain(21)
}
