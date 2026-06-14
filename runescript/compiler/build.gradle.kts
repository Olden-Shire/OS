plugins {
    application
}

dependencies {
    implementation(project(":frontend"))
}

application {
    mainClass.set("com.os.runescript.compiler.MainKt")
}
