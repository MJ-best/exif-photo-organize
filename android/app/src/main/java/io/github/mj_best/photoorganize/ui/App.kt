package io.github.mj_best.photoorganize.ui

import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.material3.SnackbarHostState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.remember
import io.github.mj_best.photoorganize.BuildConfig
import io.github.mj_best.photoorganize.core.Photo
import io.github.mj_best.photoorganize.core.View
import io.github.mj_best.photoorganize.data.LocalImages

/** The app: the library, the photo view and the import, wired to [AppViewModel]. */
@Composable
fun PhotoOrganizeApp(vm: AppViewModel) {
    val snackbar = remember { SnackbarHostState() }
    val pickCard = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocumentTree()) { uri ->
        if (uri != null) vm.cardPicked(uri)
    }
    LaunchedEffect(vm.message) {
        vm.message?.let {
            vm.message = null
            snackbar.showSnackbar(it)
        }
    }

    val library = remember(vm) {
        object : LibraryActions {
            override fun show(view: View) = vm.show(view)
            override fun open(id: Long) = vm.openViewer(id)
            override fun toggleSelect(id: Long) = vm.toggleSelect(id)
            override fun selectAll() = vm.selectAll()
            override fun clearSelection() = vm.clearSelection()
            override fun rate(ids: Collection<Long>, rating: Int) = vm.rate(ids, rating)
            override fun flag(ids: Collection<Long>, flag: Int) = vm.flag(ids, flag)
            override fun delete(ids: Collection<Long>) = vm.askDelete(ids)
            override fun import() = vm.openImport()
            override fun settings() {
                vm.loadTrashSize()
                vm.settingsOpen = true
            }
        }
    }
    val viewer = remember(vm) {
        object : ViewerActions {
            override fun close() = vm.closeViewer()
            override fun moved(index: Int) = vm.viewerAt(index)
            override fun rate(id: Long, rating: Int) = vm.rate(listOf(id), rating)
            override fun flag(id: Long, flag: Int) = vm.flag(listOf(id), flag)
            override fun delete(id: Long) = vm.askDelete(listOf(id))
        }
    }
    val import = remember(vm) {
        object : ImportActions {
            override fun pick() = pickCard.launch(null)
            override fun includeVideos(on: Boolean) = vm.includeVideos(on)
            override fun start() = vm.startImport()
            override fun cancel() = vm.cancelImport()
            override fun close() = vm.closeImport()
        }
    }

    CompositionLocalProvider(LocalImages provides vm.images) {
        val v = vm.viewer
        when {
            vm.screen == Screen.IMPORT -> {
                BackHandler { vm.closeImport() }
                ImportScreen(vm.importUi, import)
            }
            v != null -> {
                BackHandler { vm.closeViewer() }
                // Photos of the open view, read again when they change.
                val photos = remember { mutableStateMapOf<Long, Photo?>() }
                val version = vm.version
                LaunchedEffect(version, v.index) {
                    val around = (v.index - 1..v.index + 1).mapNotNull { v.ids.getOrNull(it) }
                    for (id in around) photos[id] = vm.photo(id)
                }
                ViewerScreen(
                    state = v,
                    photo = { id -> photos[id] ?: vm.ui.photos.firstOrNull { it.id == id } },
                    threshold = vm.ui.threshold,
                    act = viewer,
                    snackbar = snackbar,
                )
            }
            else -> {
                if (vm.selection.isNotEmpty()) BackHandler { vm.clearSelection() }
                LibraryScreen(vm.ui, vm.selection, library, snackbar)
            }
        }
        vm.deleteAsk?.let { DeleteDialog(it, vm::confirmDelete, vm::cancelDelete) }
        if (vm.settingsOpen) {
            SettingsDialog(
                threshold = vm.ui.threshold,
                learn = vm.ui.learn,
                trashBytes = vm.trashBytes,
                libraryPath = vm.libraryPath,
                version = BuildConfig.VERSION_NAME,
                onThreshold = vm::setThreshold,
                onEmptyTrash = vm::emptyTrash,
                onDismiss = { vm.settingsOpen = false },
            )
        }
    }
}
