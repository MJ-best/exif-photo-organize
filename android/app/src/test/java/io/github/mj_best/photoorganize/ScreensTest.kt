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
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

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
            ViewerScreen(ViewerState(ids, i), { id -> all.firstOrNull { it.id == id } }, 60, NoViewerActions, SnackbarHostState())
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

    /** The store's feature graphic (1024 × 500). */
    @Test
    @Config(qualifiers = FEATURE)
    fun featureGraphic() {
        val ids = lib.photos(View.ALL).filter { (it.bPct ?: 100) < 30 }.take(3).map { it.id }
        val images = Preloaded(ids)
        show(images) { FeatureGraphic(ids.map { images.thumbnail(it)!! }) }
        compose.onRoot().captureRoboImage("build/outputs/roborazzi/feature-graphic.png")
    }

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
const val FEATURE = "w512dp-h250dp-normal-notlong-notround-any-xhdpi-keyshidden-nonav"

@Composable
private fun FeatureGraphic(photos: List<ImageBitmap>) {
    val p = io.github.mj_best.photoorganize.ui.LocalPalette.current
    androidx.compose.foundation.layout.Box(
        androidx.compose.ui.Modifier.fillMaxSize().background(
            androidx.compose.ui.graphics.Brush.horizontalGradient(listOf(p.sidebar, p.content)),
        ),
    ) {
        androidx.compose.foundation.layout.Row(
            androidx.compose.ui.Modifier.fillMaxSize().padding(horizontal = 30.dp),
            verticalAlignment = androidx.compose.ui.Alignment.CenterVertically,
        ) {
            androidx.compose.foundation.Image(
                androidx.compose.ui.res.painterResource(R.mipmap.ic_launcher_foreground), null,
                androidx.compose.ui.Modifier.size(124.dp),
            )
            androidx.compose.foundation.layout.Spacer(androidx.compose.ui.Modifier.size(14.dp))
            androidx.compose.foundation.layout.Column(androidx.compose.ui.Modifier.weight(1f)) {
                androidx.compose.material3.Text("Photo Organize", color = p.text, fontSize = 24.sp, fontWeight = androidx.compose.ui.text.font.FontWeight.Bold, softWrap = false)
                androidx.compose.foundation.layout.Spacer(androidx.compose.ui.Modifier.height(8.dp))
                androidx.compose.material3.Text("흔들린 컷은 먼저,\n고르는 건 사진가가", color = p.text, fontSize = 16.sp, lineHeight = 22.sp, softWrap = false)
                androidx.compose.foundation.layout.Spacer(androidx.compose.ui.Modifier.height(10.dp))
                androidx.compose.material3.Text("카드에서 바로 · 기기 안에서만", color = p.text2, fontSize = 11.sp, softWrap = false)
            }
            androidx.compose.foundation.layout.Box(androidx.compose.ui.Modifier.size(118.dp, 150.dp)) {
                photos.forEachIndexed { i, img ->
                    androidx.compose.foundation.Image(
                        img, null,
                        androidx.compose.ui.Modifier
                            .align(androidx.compose.ui.Alignment.Center)
                            .padding(start = (i * 14).dp, top = (i * 8).dp)
                            .size(84.dp)
                            .rotate(-8f + i * 8f)
                            .border(4.dp, androidx.compose.ui.graphics.Color.White)
                            .clip(androidx.compose.foundation.shape.RoundedCornerShape(3.dp)),
                        contentScale = androidx.compose.ui.layout.ContentScale.Crop,
                    )
                }
            }
        }
    }
}

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
