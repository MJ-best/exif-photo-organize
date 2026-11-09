package io.github.mj_best.photoorganize

import com.sun.jna.Library
import com.sun.jna.Native
import io.github.mj_best.photoorganize.core.FileOpener
import io.github.mj_best.photoorganize.core.ImportListener
import io.github.mj_best.photoorganize.core.ImportStatus
import io.github.mj_best.photoorganize.core.PhotoLibrary
import io.github.mj_best.photoorganize.core.ProgressListener
import io.github.mj_best.photoorganize.core.SourceEntry
import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.LinearGradient
import android.graphics.Paint
import android.graphics.Path
import android.graphics.Shader
import java.io.File
import kotlin.math.sin

/** Test photos and libraries, made with the real engine (loaded through JNA from target/debug). */
object Demo {
    private interface LibC : Library {
        fun open(path: String, flags: Int): Int
    }

    private val libc: LibC by lazy { Native.load("c", LibC::class.java) }

    /** Opens files by path, the way the app opens documents (a descriptor the engine then owns). */
    val opener = object : FileOpener {
        override fun open(id: String): Int = libc.open(id, 0)
    }

    private val skies = listOf(
        0xFF7FB7E8 to 0xFFF7E3C4,
        0xFFF29E5C to 0xFFFFE2B8,
        0xFF5B6FA8 to 0xFFE9B7C9,
        0xFF9ACDE6 to 0xFFE8F2EA,
        0xFF40355E to 0xFFD9826B,
    )
    private val hills = listOf(0xFF3E7C59, 0xFF2F5D7C, 0xFF7A4E8C, 0xFFA4513F, 0xFF55704B)

    /** A landscape: sky, sun, two hills and a tower; `soft` blurs it the way a missed focus does. */
    fun landscape(seed: Int, soft: Boolean = false, w: Int = 1200, h: Int = 800): Bitmap {
        val bmp = Bitmap.createBitmap(w, h, Bitmap.Config.ARGB_8888)
        val c = Canvas(bmp)
        val paint = Paint(Paint.ANTI_ALIAS_FLAG)
        val (top, bottom) = skies[seed % skies.size]
        paint.shader = LinearGradient(0f, 0f, 0f, h * 0.7f, top.toInt(), bottom.toInt(), Shader.TileMode.CLAMP)
        c.drawRect(0f, 0f, w.toFloat(), h.toFloat(), paint)
        paint.shader = null
        paint.color = 0xFFFFF6DF.toInt()
        val sx = w * (0.2f + 0.6f * ((seed * 37) % 100) / 100f)
        c.drawCircle(sx, h * 0.25f, h / 12f, paint)
        for (layer in 0..1) {
            val path = Path()
            path.moveTo(0f, h.toFloat())
            for (x in 0..w step 8) {
                val y = h * (0.58 + 0.1 * layer) + 40 * sin((x + seed * 90 + layer * 300) / (140.0 + layer * 60))
                path.lineTo(x.toFloat(), y.toFloat())
            }
            path.lineTo(w.toFloat(), h.toFloat())
            path.close()
            paint.color = hills[(seed + layer) % hills.size].toInt()
            if (layer == 0) paint.alpha = 200
            c.drawPath(path, paint)
            paint.alpha = 255
        }
        // A tower with fine lines: detail for the focus measure.
        paint.color = Color.rgb(0x2B, 0x21, 0x1C)
        paint.strokeWidth = 2f
        val tx = w * (0.15f + 0.7f * ((seed * 53) % 100) / 100f)
        c.drawRect(tx, h * 0.45f, tx + w / 40f, h * 0.7f, paint)
        for (i in 0 until 60 step 3) c.drawLine(tx - 30 + i, h * 0.45f, tx + i % 7, h * 0.40f, paint)
        if (!soft) return bmp
        val small = Bitmap.createScaledBitmap(bmp, w / 16, h / 16, true)
        return Bitmap.createScaledBitmap(small, w, h, true)
    }

    fun writeJpeg(bmp: Bitmap, file: File) {
        file.outputStream().use { bmp.compress(Bitmap.CompressFormat.JPEG, 90, it) }
    }

    /** A card: `n` photos in DCIM/100TEST, every fourth one out of focus. */
    fun card(dir: File, n: Int, startMs: Long = 1_780_300_000_000): List<SourceEntry> {
        val dcim = File(dir, "DCIM/100TEST").apply { mkdirs() }
        return (1..n).map { i ->
            val f = File(dcim, "IMG_%04d.JPG".format(i))
            writeJpeg(landscape(i, soft = i % 4 == 0), f)
            val t = startMs + i * 47_000L + (i / 8) * 86_400_000L
            f.setLastModified(t)
            SourceEntry(rel = "DCIM/100TEST/${f.name}", size = f.length(), mtimeMs = t, id = f.path)
        }
    }

    /** A library with these photos imported and looked at by the culling assistant. */
    fun library(root: File, card: List<SourceEntry>): PhotoLibrary {
        val lib = PhotoLibrary.open(root.path)
        lib.scan("EOS_DIGITAL", card, opener, true)
        lib.importScanned(object : ImportListener {
            override fun progress(status: ImportStatus) {}
        })
        lib.analyze(object : ProgressListener {
            override fun progress(done: Int, total: Int) {}
        })
        return lib
    }
}
