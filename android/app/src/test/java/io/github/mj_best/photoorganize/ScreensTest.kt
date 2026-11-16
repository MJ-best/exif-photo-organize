package io.github.mj_best.photoorganize

import android.graphics.BitmapFactory
import androidx.compose.material3.SnackbarHostState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onRoot
import com.github.takahirom.roborazzi.RobolectricDeviceQualifiers
import com.github.takahirom.roborazzi.captureRoboImage
import io.github.mj_best.photoorganize.core.ImportResult
import io.github.mj_best.photoorganize.core.ImportStatus
import io.github.mj_best.photoorganize.core.Photo
import io.github.mj_best.photoorganize.core.PhotoLibrary
import io.github.mj_best.photoorganize.core.ScanInfo
import io.github.mj_best.photoorganize.core.View
import io.github.mj_best.photoorganize.data.ImageSource
import io.github.mj_best.photoorganize.data.LocalImages
import io.github.mj_best.photoorganize.ui.ImportActions
import io.github.mj_best.photoorganize.ui.ImportScreen
import io.github.mj_best.photoorganize.ui.ImportUi
import io.github.mj_best.photoorganize.ui.LibraryActions
import io.github.mj_best.photoorganize.ui.LibraryScreen
import io.github.mj_best.photoorganize.ui.LibraryUi
import io.github.mj_best.photoorganize.ui.NoCounts
import io.github.mj_best.photoorganize.ui.PhotoTheme
import io.github.mj_best.photoorganize.ui.ViewerActions
import io.github.mj_best.photoorganize.ui.ViewerScreen
import io.github.mj_best.photoorganize.ui.ViewerState
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import java.io.File
import java.nio.file.Files

/**
 * The screens with a real library: one made here from test photos, or the one given with
 * `-Pdemo=<library folder>` (copied first). Screenshots go to build/outputs/roborazzi.
 */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class ScreensTest {
    @get:Rule val compose = createComposeRule()

    private object Lib {
        val lib: PhotoLibrary by lazy {
            val dir = Files.createTempDirectory("photoorg-screens").toFile()
            val demo = System.getProperty("photoorg.demo")?.let(::File)
            if (demo != null && demo.isDirectory) {
                demo.copyRecursively(File(dir, "Library"))
                PhotoLibrary.open(File(dir, "Library").path).also { it.learn() }
            } else {
                Demo.library(File(dir, "Library"), Demo.card(File(dir, "card"), 40))
            }
        }
    }

    private val lib get() = Lib.lib

    /** Images decoded up front, so the screenshot has them. */
    private inner class Preloaded(ids: Collection<Long>, private val big: Long? = null) : ImageSource {
        private val thumbs: Map<Long, ImageBitmap> = ids.associateWith { decode(lib.thumbnail(it)!!) }
        private val preview: ImageBitmap? = big?.let { decode(lib.preview(it, 1600)) }

        private fun decode(b: ByteArray) = BitmapFactory.decodeByteArray(b, 0, b.size).asImageBitmap()

        override fun thumbnail(id: Long) = thumbs[id]
        override fun preview(id: Long, maxEdge: Int) = if (id == big) preview else thumbs[id]
        override fun cached(id: Long) = thumbs[id]
        override fun cachedPreview(id: Long) = if (id == big) preview else null
    }

    private fun ui(view: View): LibraryUi = LibraryUi(
        ready = true,
        view = view,
        photos = lib.photos(view),
        counts = lib.counts(),
        learn = lib.learnStatus(),
        threshold = 60,
    )

    private fun show(images: ImageSource, content: @Composable () -> Unit) {
        compose.setContent {
            PhotoTheme(dark = false) {
                CompositionLocalProvider(LocalImages provides images) { content() }
            }
        }
        compose.waitForIdle()
    }

    private fun library(view: View, file: String, selection: Set<Long> = emptySet()) {
        val ui = ui(view)
        assertTrue("$view has photos", ui.photos.isNotEmpty())
        show(Preloaded(ui.photos.take(80).map { it.id })) {
            LibraryScreen(ui, selection, NoLibraryActions, SnackbarHostState())
        }
        compose.onRoot().captureRoboImage("build/outputs/roborazzi/$file")
    }

    private fun viewer(file: String) {
        val all = lib.photos(View.ALL)
        // A photo the assistant has something to say about.
        val p: Photo = all.filter { it.reasons.isNotEmpty() }.maxByOrNull { it.bPct ?: 0 } ?: all.first()
        val ids = all.map { it.id }
        val i = ids.indexOf(p.id)
        show(Preloaded(ids.subList((i - 1).coerceAtLeast(0), (i + 2).coerceAtMost(ids.size)), big = p.id)) {
            ViewerScreen(
                ViewerState(ids, i),
                { id -> lib.photo(id) ?: all.firstOrNull { it.id == id } },
                60,
                NoViewerActions,
                SnackbarHostState(),
            )
        }
        compose.onRoot().captureRoboImage("build/outputs/roborazzi/$file")
    }

    @Test
    @Config(qualifiers = RobolectricDeviceQualifiers.MediumTablet)
    fun libraryTablet() = library(View.ALL, "library-tablet.png")

    @Test
    @Config(qualifiers = PHONE)
    fun libraryPhone() = library(View.ALL, "library-phone.png")

    @Test
    @Config(qualifiers = RobolectricDeviceQualifiers.MediumTablet)
    fun candidatesTablet() = library(View.CANDIDATES, "candidates-tablet.png")

    @Test
    @Config(qualifiers = PHONE)
    fun candidatesPhone() = library(View.CANDIDATES, "candidates-phone.png")

    @Test
    @Config(qualifiers = PHONE)
    fun selectionPhone() {
        val ids = lib.photos(View.ALL).take(3).map { it.id }.toSet()
        library(View.ALL, "selection-phone.png", ids)
    }

    @Test
    @Config(qualifiers = PHONE)
    fun viewerPhone() = viewer("viewer-phone.png")

    @Test
    @Config(qualifiers = RobolectricDeviceQualifiers.MediumTablet)
    fun viewerTablet() = viewer("viewer-tablet.png")

    @Test
    @Config(qualifiers = TABLET7)
    fun tablet7() {
        library(View.ALL, "library-tablet7.png")
    }

    @Test
    @Config(qualifiers = TABLET7)
    fun viewerTablet7() = viewer("viewer-tablet7.png")

    @Test
    @Config(qualifiers = PHONE)
    fun welcomePhone() {
        show(Preloaded(emptyList())) {
            LibraryScreen(LibraryUi(ready = true, counts = NoCounts), emptySet(), NoLibraryActions, SnackbarHostState())
        }
        compose.onRoot().captureRoboImage("build/outputs/roborazzi/welcome-phone.png")
    }

    @Test
    @Config(qualifiers = PHONE)
    fun importPhone() {
        val day = 86_400_000L
        val info = ScanInfo(
            label = "SanDisk SD 카드 · DCIM", photos = 412, newPhotos = 386, newFiles = 771,
            newBytes = 18_420_000_000, videos = 6, rawPairs = 379,
            firstMs = 1_791_100_000_000, lastMs = 1_791_100_000_000 + 2 * day,
            cameras = listOf("Sony ILCE-7M4 · 380장", "iPhone 15 Pro · 32장"), errors = emptyList(),
        )
        var state by mutableStateOf<ImportUi>(ImportUi.Ready(info, true, 96_000_000_000))
        show(Preloaded(emptyList())) { ImportScreen(state, NoImportActions) }
        compose.onRoot().captureRoboImage("build/outputs/roborazzi/import-ready-phone.png")
        state = ImportUi.Copying(ImportStatus(312, 771, 7_450_000_000, 18_420_000_000, "DSC01241.ARW"), info.label)
        compose.waitForIdle()
        compose.onRoot().captureRoboImage("build/outputs/roborazzi/import-copying-phone.png")
        state = ImportUi.Done(ImportResult(386, 0, 26, 18_420_000_000, emptyList(), false))
        compose.waitForIdle()
        compose.onRoot().captureRoboImage("build/outputs/roborazzi/import-done-phone.png")
    }
}

/** Store screenshot sizes: phone 1080 × 1920, 7" tablet 1200 × 1920; 10" is MediumTablet (2560 × 1600). */
const val PHONE = "w360dp-h640dp-normal-notlong-notround-any-xxhdpi-keyshidden-nonav"
const val TABLET7 = "w600dp-h960dp-large-notlong-notround-any-xhdpi-keyshidden-nonav"

private object NoLibraryActions : LibraryActions {
    override fun show(view: View) {}
    override fun open(id: Long) {}
    override fun toggleSelect(id: Long) {}
    override fun selectAll() {}
    override fun clearSelection() {}
    override fun rate(ids: Collection<Long>, rating: Int) {}
    override fun flag(ids: Collection<Long>, flag: Int) {}
    override fun delete(ids: Collection<Long>) {}
    override fun import() {}
    override fun settings() {}
}

private object NoViewerActions : ViewerActions {
    override fun close() {}
    override fun moved(index: Int) {}
    override fun rate(id: Long, rating: Int) {}
    override fun flag(id: Long, flag: Int) {}
    override fun delete(id: Long) {}
}

private object NoImportActions : ImportActions {
    override fun pick() {}
    override fun includeVideos(on: Boolean) {}
    override fun start() {}
    override fun cancel() {}
    override fun close() {}
}
