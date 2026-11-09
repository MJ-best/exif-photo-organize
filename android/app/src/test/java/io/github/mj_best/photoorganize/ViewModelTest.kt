package io.github.mj_best.photoorganize

import android.os.Looper
import androidx.test.core.app.ApplicationProvider
import io.github.mj_best.photoorganize.core.PhotoLibrary
import io.github.mj_best.photoorganize.core.View
import io.github.mj_best.photoorganize.ui.AppViewModel
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.GraphicsMode
import java.io.File

/** The app's state with a real library: what the screens see after each action. */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class ViewModelTest {
    @get:Rule val tmp = TemporaryFolder()

    /** Run the main thread until [done] (the engine works on background threads). */
    private fun until(what: String, done: () -> Boolean) {
        val end = System.currentTimeMillis() + 30_000
        while (!done()) {
            shadowOf(Looper.getMainLooper()).idle()
            check(System.currentTimeMillis() < end) { "timed out: $what" }
            Thread.sleep(20)
        }
    }

    @Test
    fun opensRatesFiltersAndDeletes() {
        val dir = File(tmp.root, "Library")
        Demo.library(dir, Demo.card(tmp.newFolder("card"), 16)).close()

        val app = ApplicationProvider.getApplicationContext<PhotoApp>()
        val vm = AppViewModel(app) { PhotoLibrary.open(dir.path) }
        until("library opened") { vm.ui.ready }
        assertNull(vm.ui.error)
        assertEquals(16, vm.ui.photos.size)
        assertEquals(16, vm.ui.counts.analysed)
        assertTrue(vm.ui.counts.candidates > 0)

        // The B-cut candidates, then reject them all, as the bar's button does.
        vm.show(View.CANDIDATES)
        until("candidates") { vm.ui.view == View.CANDIDATES && vm.ui.photos.isNotEmpty() && vm.ui.photos.all { (it.bPct ?: 0) >= 60 } }
        val candidates = vm.ui.photos.map { it.id }
        vm.flag(candidates, -1)
        until("rejected") { vm.ui.counts.rejected == candidates.size }
        until("candidates left the view") { vm.ui.photos.isEmpty() }

        // Stars from the photo view; the view stays put while it is open.
        vm.show(View.ALL)
        until("all") { vm.ui.view == View.ALL && vm.ui.photos.size == 16 }
        val first = vm.ui.photos.first().id
        vm.openViewer(first)
        assertNotNull(vm.viewer)
        vm.rate(listOf(first), 4)
        until("rated") { vm.version > 0 }
        vm.closeViewer()
        until("rating shown") { vm.ui.counts.rated == 1 }

        // Delete the rejected photos.
        vm.askDelete(candidates)
        until("asked") { vm.deleteAsk != null }
        assertEquals(candidates.size, vm.deleteAsk!!.plan.photos)
        vm.confirmDelete()
        until("deleted") { vm.ui.counts.all == 16 - candidates.size }
        assertEquals(0, vm.ui.counts.rejected)
        assertTrue(vm.message!!.contains("휴지통"))
    }
}
