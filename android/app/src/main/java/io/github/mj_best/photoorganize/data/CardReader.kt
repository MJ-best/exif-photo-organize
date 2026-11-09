package io.github.mj_best.photoorganize.data

import android.content.ContentResolver
import android.content.Context
import android.net.Uri
import android.os.storage.StorageManager
import android.provider.DocumentsContract
import android.provider.DocumentsContract.Document
import androidx.core.net.toUri
import io.github.mj_best.photoorganize.core.FileOpener
import io.github.mj_best.photoorganize.core.SourceEntry
import io.github.mj_best.photoorganize.core.isMediaName

/**
 * Reads a card (or any folder) picked with the system folder picker. Android hands out documents,
 * not paths: the card is listed here, and the engine opens each file through [Opener].
 */
class CardReader(private val context: Context) {
    data class Listing(val label: String, val files: List<SourceEntry>)

    /** Every photo, video and XMP sidecar under [tree]. */
    fun list(tree: Uri, cancelled: () -> Boolean = { false }): Listing {
        val resolver = context.contentResolver
        val rootId = DocumentsContract.getTreeDocumentId(tree)
        val out = ArrayList<SourceEntry>()
        val stack = ArrayDeque<Pair<String, String>>()
        stack.addLast(rootId to "")
        while (stack.isNotEmpty() && !cancelled()) {
            val (docId, prefix) = stack.removeLast()
            val children = DocumentsContract.buildChildDocumentsUriUsingTree(tree, docId)
            resolver.query(children, PROJECTION, null, null, null)?.use { c ->
                while (c.moveToNext()) {
                    val id = c.getString(0) ?: continue
                    val name = c.getString(1) ?: continue
                    if (name.startsWith(".")) continue
                    val rel = if (prefix.isEmpty()) name else "$prefix/$name"
                    if (c.getString(2) == Document.MIME_TYPE_DIR) {
                        if (rel.count { it == '/' } < 16) stack.addLast(id to rel)
                    } else if (isMediaName(name) || name.endsWith(".xmp", ignoreCase = true)) {
                        out += SourceEntry(
                            rel = rel,
                            size = if (c.isNull(3)) 0L else c.getLong(3),
                            mtimeMs = if (c.isNull(4)) null else c.getLong(4),
                            id = DocumentsContract.buildDocumentUriUsingTree(tree, id).toString(),
                        )
                    }
                }
            }
        }
        return Listing(label(tree, rootId), out)
    }

    /** "SanDisk SD 카드 · DCIM", or the folder's name. */
    private fun label(tree: Uri, rootId: String): String {
        val folder = runCatching {
            context.contentResolver.query(
                DocumentsContract.buildDocumentUriUsingTree(tree, rootId),
                arrayOf(Document.COLUMN_DISPLAY_NAME), null, null, null,
            )?.use { c -> if (c.moveToFirst()) c.getString(0) else null }
        }.getOrNull()
        val uuid = rootId.substringBefore(':', "")
        val volume = runCatching {
            context.getSystemService(StorageManager::class.java)?.storageVolumes
                ?.firstOrNull { it.uuid != null && it.uuid.equals(uuid, ignoreCase = true) }
                ?.getDescription(context)
        }.getOrNull()
        return listOfNotNull(volume, folder?.takeIf { it.isNotBlank() }).distinct().joinToString(" · ").ifBlank { "카드" }
    }

    class Opener(private val resolver: ContentResolver) : FileOpener {
        override fun open(id: String): Int = try {
            resolver.openFileDescriptor(id.toUri(), "r")?.detachFd() ?: -1
        } catch (e: Exception) {
            -1
        }
    }

    fun opener(): FileOpener = Opener(context.contentResolver)

    private companion object {
        val PROJECTION = arrayOf(
            Document.COLUMN_DOCUMENT_ID,
            Document.COLUMN_DISPLAY_NAME,
            Document.COLUMN_MIME_TYPE,
            Document.COLUMN_SIZE,
            Document.COLUMN_LAST_MODIFIED,
        )
    }
}
