package io.github.mj_best.photoorganize

import android.app.Application
import java.io.File

class PhotoApp : Application() {
    /**
     * The library: the app's own folder on the shared storage (no permission needed). It goes
     * away with the app, which the settings say.
     */
    val libraryDir: File by lazy { File(getExternalFilesDir(null) ?: filesDir, "Library") }
}
