# The engine is called through JNA (UniFFI bindings): keep JNA and the generated bindings, which
# JNA reaches by reflection.
-keep class com.sun.jna.** { *; }
-keepclassmembers class * extends com.sun.jna.** { public *; }
-keep class io.github.mj_best.photoorganize.core.** { *; }
-dontwarn java.awt.**
