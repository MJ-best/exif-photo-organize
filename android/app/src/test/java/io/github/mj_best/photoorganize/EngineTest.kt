package io.github.mj_best.photoorganize

import io.github.mj_best.photoorganize.core.ImportListener
import io.github.mj_best.photoorganize.core.ImportStatus
import io.github.mj_best.photoorganize.core.PhotoException
import io.github.mj_best.photoorganize.core.PhotoLibrary
import io.github.mj_best.photoorganize.core.ProgressListener
import io.github.mj_best.photoorganize.core.View
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.GraphicsMode
import java.io.File

/** The engine through the generated Kotlin bindings, as the app calls it. */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class EngineTest {
    @get:Rule val tmp = TemporaryFolder()

    @Test
    fun importRateCullAndDelete() {
        val card = Demo.card(tmp.newFolder("card"), 12)
        val lib = PhotoLibrary.open(File(tmp.root, "Library").path)
        assertEquals(0, lib.counts().all)

        val info = lib.scan("EOS_DIGITAL", card, Demo.opener, true)
        assertEquals(12, info.newPhotos)
        assertEquals(12, info.newFiles)
        assertTrue(info.newBytes > 0)

        val seen = mutableListOf<ImportStatus>()
        val res = lib.importScanned(object : ImportListener {
            override fun progress(status: ImportStatus) {
                synchronized(seen) { seen += status }
            }
        })
        assertEquals(res.failures.toString(), 12, res.added)
        assertEquals(12, seen.last().filesDone)
        val photos = lib.photos(View.ALL)
        assertEquals(12, photos.size)
        assertNotNull(lib.thumbnail(photos[0].id))
        assertTrue(lib.preview(photos[0].id, 800).size > 1000)

        // Again: nothing new.
        assertEquals(0, lib.scan("EOS_DIGITAL", card, Demo.opener, true).newPhotos)

        lib.setRating(listOf(photos[0].id), 5)
        lib.setFlag(listOf(photos[1].id, photos[2].id), -1)
        val c = lib.counts()
        assertEquals(1, c.rated)
        assertEquals(2, c.rejected)
        assertEquals(9, c.undecided)

        var last = 0 to 0
        assertEquals(12, lib.analyze(object : ProgressListener {
            override fun progress(done: Int, total: Int) {
                last = done to total
            }
        }))
        assertEquals(12 to 12, last)
        assertEquals(12, lib.counts().analysed)
        val soft = lib.photos(View.ALL).filter { it.name.substringAfter('_').toInt() % 4 == 0 }
        val sharp = lib.photos(View.ALL).filter { it.name.substringAfter('_').toInt() % 4 != 0 }
        assertTrue("blurred photos look like B-cuts", soft.minOf { it.bPct!! } > sharp.minOf { it.bPct!! })
        assertTrue(soft.all { "초점 흐림" in it.reasons })

        val plan = lib.deletePlan(listOf(photos[1].id))
        assertEquals(1, plan.photos)
        val del = lib.delete(listOf(photos[1].id))
        assertEquals(1, del.deleted)
        assertNull(lib.photo(photos[1].id))
        assertTrue(lib.trashBytes() > 0)
        lib.emptyTrash()
        assertEquals(0L, lib.trashBytes())
    }

    @Test
    fun errorsArriveAsExceptions() {
        val file = tmp.newFile("not-a-folder")
        val e = runCatching { PhotoLibrary.open(File(file, "Library").path) }.exceptionOrNull()
        assertTrue(e is PhotoException.Failed && e.reason.isNotBlank())
    }
}
