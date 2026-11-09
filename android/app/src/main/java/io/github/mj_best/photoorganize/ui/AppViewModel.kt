package io.github.mj_best.photoorganize.ui

import android.app.Application
import android.content.Context
import android.net.Uri
import android.os.StatFs
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.core.content.edit
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import io.github.mj_best.photoorganize.PhotoApp
import io.github.mj_best.photoorganize.core.Counts
import io.github.mj_best.photoorganize.core.DeletePlan
import io.github.mj_best.photoorganize.core.ImportListener
import io.github.mj_best.photoorganize.core.ImportResult
import io.github.mj_best.photoorganize.core.ImportStatus
import io.github.mj_best.photoorganize.core.LearnStatus
import io.github.mj_best.photoorganize.core.Photo
import io.github.mj_best.photoorganize.core.PhotoException
import io.github.mj_best.photoorganize.core.PhotoLibrary
import io.github.mj_best.photoorganize.core.PhotoLibraryInterface
import io.github.mj_best.photoorganize.core.ProgressListener
import io.github.mj_best.photoorganize.core.ScanInfo
import io.github.mj_best.photoorganize.core.View
import io.github.mj_best.photoorganize.data.CardReader
import io.github.mj_best.photoorganize.data.EngineImages
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.io.File

enum class Screen { LIBRARY, IMPORT }

val NoCounts = Counts(0, 0, 0, 0, 0, 0, 0, 0)

data class LibraryUi(
    val ready: Boolean = false,
    val error: String? = null,
    val view: View = View.ALL,
    val photos: List<Photo> = emptyList(),
    val counts: Counts = NoCounts,
    val learn: LearnStatus? = null,
    /** The culling assistant looking at new photos: done / total. */
    val analysis: Pair<Int, Int>? = null,
    val threshold: Int = 60,
)

sealed interface ImportUi {
    data object Idle : ImportUi
    data object Listing : ImportUi
    data class Ready(val info: ScanInfo, val includeVideos: Boolean, val freeBytes: Long) : ImportUi
    data class Copying(val status: ImportStatus, val label: String) : ImportUi
    data class Done(val result: ImportResult) : ImportUi
    data class Failed(val message: String) : ImportUi
}

/** The photo view: the photos it pages through (fixed while it is open) and where it is. */
data class ViewerState(val ids: List<Long>, val index: Int)

data class DeleteAsk(val ids: List<Long>, val plan: DeletePlan)

class AppViewModel(
    app: Application,
    private val openLibrary: (File) -> PhotoLibraryInterface,
) : AndroidViewModel(app) {
    constructor(app: Application) : this(app, { dir -> PhotoLibrary.open(dir.path) })

    private val io = Dispatchers.IO
    private val dir: File = (app as PhotoApp).libraryDir
    private val prefs = app.getSharedPreferences("settings", Context.MODE_PRIVATE)
    private val reader = CardReader(app)
    private var lib: PhotoLibraryInterface? = null

    val images = EngineImages(
        load = { id -> lib?.thumbnail(id) },
        big = { id, edge -> lib?.preview(id, edge) },
    )

    var ui by mutableStateOf(LibraryUi(threshold = prefs.getInt("threshold", 60)))
        private set
    var screen by mutableStateOf(Screen.LIBRARY)
        private set
    var importUi by mutableStateOf<ImportUi>(ImportUi.Idle)
        private set
    var viewer by mutableStateOf<ViewerState?>(null)
        private set
    var selection by mutableStateOf<Set<Long>>(emptySet())
        private set
    var deleteAsk by mutableStateOf<DeleteAsk?>(null)
        private set
    var message by mutableStateOf<String?>(null)
    var settingsOpen by mutableStateOf(false)
    var trashBytes by mutableLongStateOf(0L)
        private set

    /** Bumped when photos change, so open views read them again. */
    var version by mutableIntStateOf(0)
        private set

    private var lastTree: Uri? = null
    private var learnJob: Job? = null
    private var analysisJob: Job? = null
    private var importJob: Job? = null

    val libraryPath: String get() = dir.path

    init {
        viewModelScope.launch {
            val opened = withContext(io) {
                runCatching {
                    dir.mkdirs()
                    openLibrary(dir).also { it.setThreshold(ui.threshold) }
                }
            }
            opened.onSuccess {
                lib = it
                refresh()
                analyse()
            }.onFailure { ui = ui.copy(error = errorText(it)) }
        }
    }

    // --- Library ------------------------------------------------------------------------------

    fun show(view: View) {
        if (view == ui.view) return
        ui = ui.copy(view = view)
        selection = emptySet()
        refresh()
    }

    fun refresh() {
        val l = lib ?: return
        val view = ui.view
        viewModelScope.launch {
            val (photos, counts, learn) = withContext(io) { Triple(l.photos(view), l.counts(), l.learnStatus()) }
            if (ui.view == view) {
                ui = ui.copy(ready = true, photos = photos, counts = counts, learn = learn)
            }
        }
    }

    suspend fun photo(id: Long): Photo? = withContext(io) { lib?.photo(id) }

    fun rate(ids: Collection<Long>, rating: Int) = change(ids) { it.setRating(ids.toList(), rating) }

    /** 1 pick, -1 reject, 0 clear. */
    fun flag(ids: Collection<Long>, flag: Int) = change(ids) { it.setFlag(ids.toList(), flag) }

    private fun change(ids: Collection<Long>, block: (PhotoLibraryInterface) -> Unit) {
        val l = lib ?: return
        if (ids.isEmpty()) return
        viewModelScope.launch {
            runCatching { withContext(io) { block(l) } }.onFailure { message = "저장하지 못했습니다: ${errorText(it)}" }
            version++
            if (viewer == null) refresh()
            learnSoon()
        }
    }

    /** Decisions are learned right away, a moment after the last one. */
    private fun learnSoon() {
        val l = lib ?: return
        learnJob?.cancel()
        learnJob = viewModelScope.launch {
            delay(1500)
            withContext(io) { runCatching { l.learn() } }
            version++
            if (viewer == null) refresh()
        }
    }

    // --- Selection ----------------------------------------------------------------------------

    fun toggleSelect(id: Long) {
        selection = if (id in selection) selection - id else selection + id
    }

    fun selectAll() {
        selection = ui.photos.map { it.id }.toSet()
    }

    fun clearSelection() {
        selection = emptySet()
    }

    // --- Photo view ---------------------------------------------------------------------------

    fun openViewer(id: Long) {
        val ids = ui.photos.map { it.id }
        val i = ids.indexOf(id)
        if (i >= 0) viewer = ViewerState(ids, i)
    }

    fun viewerAt(index: Int) {
        viewer = viewer?.copy(index = index)
    }

    fun closeViewer() {
        viewer = null
        refresh()
    }

    // --- Culling assistant --------------------------------------------------------------------

    /** Look at photos the assistant has not seen yet, then learn and score. */
    fun analyse() {
        val l = lib ?: return
        if (analysisJob?.isActive == true) return
        analysisJob = viewModelScope.launch {
            val pending = withContext(io) { l.pendingAnalysis() }
            if (pending == 0) {
                withContext(io) { runCatching { l.learn() } }
                refresh()
                return@launch
            }
            ui = ui.copy(analysis = 0 to pending)
            var scoredAt = 0
            val listener = object : ProgressListener {
                override fun progress(done: Int, total: Int) {
                    viewModelScope.launch(Dispatchers.Main) {
                        ui = ui.copy(analysis = done to total)
                        // Use what is ready while a long analysis goes on.
                        if (done - scoredAt >= 300) {
                            scoredAt = done
                            launch(io) { runCatching { l.learn() } }.invokeOnCompletion { refreshOnMain() }
                        }
                    }
                }
            }
            runCatching { withContext(io) { l.analyze(listener) } }
                .onFailure { message = "셀렉 도우미: ${errorText(it)}" }
            ui = ui.copy(analysis = null)
            version++
            refresh()
        }
    }

    private fun refreshOnMain() {
        viewModelScope.launch(Dispatchers.Main) { if (viewer == null) refresh() }
    }

    fun setThreshold(percent: Int) {
        val p = percent.coerceIn(30, 90)
        prefs.edit { putInt("threshold", p) }
        ui = ui.copy(threshold = p)
        lib?.setThreshold(p)
        refresh()
    }

    // --- Import -------------------------------------------------------------------------------

    fun openImport() {
        if (importUi is ImportUi.Done || importUi is ImportUi.Failed) importUi = ImportUi.Idle
        screen = Screen.IMPORT
    }

    fun closeImport() {
        if (importUi is ImportUi.Copying) return
        screen = Screen.LIBRARY
        if (importUi !is ImportUi.Ready) importUi = ImportUi.Idle
        refresh()
    }

    fun cardPicked(tree: Uri, includeVideos: Boolean = true) {
        val l = lib ?: return
        lastTree = tree
        importUi = ImportUi.Listing
        viewModelScope.launch {
            val res = withContext(io) {
                runCatching {
                    val listing = reader.list(tree) { !isActive }
                    l.scan(listing.label, listing.files, reader.opener(), includeVideos)
                }
            }
            importUi = res.fold(
                { ImportUi.Ready(it, includeVideos, freeBytes()) },
                { ImportUi.Failed("카드를 읽지 못했습니다: ${errorText(it)}") },
            )
        }
    }

    fun includeVideos(on: Boolean) {
        lastTree?.let { cardPicked(it, on) }
    }

    fun startImport() {
        val l = lib ?: return
        val ready = importUi as? ImportUi.Ready ?: return
        importUi = ImportUi.Copying(
            ImportStatus(0, ready.info.newFiles, 0, ready.info.newBytes, ""),
            ready.info.label,
        )
        importJob = viewModelScope.launch {
            val listener = object : ImportListener {
                override fun progress(status: ImportStatus) {
                    viewModelScope.launch(Dispatchers.Main) {
                        if (importUi is ImportUi.Copying) importUi = ImportUi.Copying(status, ready.info.label)
                    }
                }
            }
            val res = withContext(io) { runCatching { l.importScanned(listener) } }
            importUi = res.fold({ ImportUi.Done(it) }, { ImportUi.Failed("가져오지 못했습니다: ${errorText(it)}") })
            refresh()
            analyse()
        }
    }

    fun cancelImport() {
        lib?.cancelImport()
    }

    private fun freeBytes(): Long = runCatching { StatFs(dir.path).availableBytes }.getOrDefault(Long.MAX_VALUE)

    // --- Delete -------------------------------------------------------------------------------

    fun askDelete(ids: Collection<Long>) {
        val l = lib ?: return
        if (ids.isEmpty()) return
        viewModelScope.launch {
            val plan = withContext(io) { l.deletePlan(ids.toList()) }
            deleteAsk = DeleteAsk(ids.toList(), plan)
        }
    }

    fun cancelDelete() {
        deleteAsk = null
    }

    fun confirmDelete() {
        val l = lib ?: return
        val ask = deleteAsk ?: return
        deleteAsk = null
        viewModelScope.launch {
            val res = withContext(io) {
                runCatching {
                    val r = l.delete(ask.ids)
                    r to ask.ids.filter { l.photo(it) == null }.toSet()
                }
            }
            res.onSuccess { (r, gone) ->
                images.forget(gone)
                selection = selection - gone
                viewer?.let { v ->
                    val ids = v.ids.filter { it !in gone }
                    viewer = if (ids.isEmpty()) null else ViewerState(ids, v.index.coerceAtMost(ids.size - 1))
                }
                if (r.deleted > 0) message = "${Fmt.count(r.deleted)}장을 휴지통으로 옮겼습니다 (${Fmt.bytes(r.bytes)})."
                if (r.failed > 0) message = "${Fmt.count(r.failed)}장은 옮기지 못했습니다: ${r.firstError ?: ""}"
            }.onFailure { message = "휴지통으로 옮기지 못했습니다: ${errorText(it)}" }
            version++
            refresh()
            learnSoon()
        }
    }

    fun loadTrashSize() {
        val l = lib ?: return
        viewModelScope.launch { trashBytes = withContext(io) { l.trashBytes() } }
    }

    fun emptyTrash() {
        val l = lib ?: return
        viewModelScope.launch {
            runCatching { withContext(io) { l.emptyTrash() } }
                .onSuccess { message = "휴지통을 비웠습니다." }
                .onFailure { message = errorText(it) }
            loadTrashSize()
        }
    }

    override fun onCleared() {
        lib?.cancelImport()
        lib?.cancelAnalysis()
    }

    private fun errorText(e: Throwable): String = when (e) {
        is PhotoException.Failed -> e.reason
        else -> e.message ?: e.toString()
    }
}
