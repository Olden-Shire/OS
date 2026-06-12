plugins {
    application
}

dependencies {
    implementation(project(":frontend"))
}

application {
    mainClass.set("com.os1.runescript.compiler.MainKt")
}
