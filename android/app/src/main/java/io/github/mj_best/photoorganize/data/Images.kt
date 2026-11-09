package io.github.mj_best.photoorganize.data

import android.graphics.BitmapFactory
import android.util.LruCache
import androidx.compose.runtime.Composable
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.produceState
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/** Where the screens get their pictures from (the engine, or made-up ones in previews and tests). */
interface ImageSource {
    /** The thumbnail made on import. */
    fun thumbnail(id: Long): ImageBitmap?

    /** A large picture for the photo view, its long edge about [maxEdge] pixels. */
    fun preview(id: Long, maxEdge: Int): ImageBitmap?

    /** A thumbnail that is ready without loading (shown at once). */
    fun cached(id: Long): ImageBitmap? = null

    fun cachedPreview(id: Long): ImageBitmap? = null
}

val LocalImages = compositionLocalOf<ImageSource> { error("no image source") }

/** Decodes the engine's JPEGs and keeps recent thumbnails in memory. */
class EngineImages(private val load: (Long) -> ByteArray?, private val big: (Long, Int) -> ByteArray?) : ImageSource {
    private val thumbs = object : LruCache<Long, ImageBitmap>(48 * 1024 * 1024) {
        override fun sizeOf(key: Long, value: ImageBitmap) = value.width * value.height * 4
    }

    override fun thumbnail(id: Long): ImageBitmap? {
        thumbs.get(id)?.let { return it }
        val bytes = load(id) ?: return null
        val bmp = BitmapFactory.decodeByteArray(bytes, 0, bytes.size)?.asImageBitmap() ?: return null
        thumbs.put(id, bmp)
        return bmp
    }

    override fun cached(id: Long): ImageBitmap? = thumbs.get(id)

    override fun preview(id: Long, maxEdge: Int): ImageBitmap? {
        val bytes = big(id, maxEdge) ?: return null
        return BitmapFactory.decodeByteArray(bytes, 0, bytes.size)?.asImageBitmap()
    }

    fun forget(ids: Collection<Long>) = ids.forEach { thumbs.remove(it) }
}

@Composable
fun rememberThumbnail(id: Long): ImageBitmap? {
    val images = LocalImages.current
    val bmp by produceState(initialValue = images.cached(id), id, images) {
        if (value == null) value = withContext(Dispatchers.IO) { images.thumbnail(id) }
    }
    return bmp
}

@Composable
fun rememberPreview(id: Long, maxEdge: Int): ImageBitmap? {
    val images = LocalImages.current
    val bmp by produceState(initialValue = images.cachedPreview(id), id, maxEdge, images) {
        if (value == null) value = withContext(Dispatchers.IO) { runCatching { images.preview(id, maxEdge) }.getOrNull() }
    }
    return bmp
}
