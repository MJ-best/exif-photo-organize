package io.github.mj_best.photoorganize.ui

import java.time.DayOfWeek
import java.time.LocalDate
import java.time.LocalDateTime
import java.time.ZoneOffset
import java.util.Locale

/** Formatting shared with the desktop app (same words, same units). */
object Fmt {
    /** Capture times are the camera's clock, kept as if it were UTC. */
    fun dateTime(capturedMs: Long): LocalDateTime =
        LocalDateTime.ofEpochSecond(Math.floorDiv(capturedMs, 1000L), 0, ZoneOffset.UTC)

    fun day(capturedMs: Long): Long = Math.floorDiv(capturedMs, 86_400_000L)

    fun weekday(d: DayOfWeek): String = when (d) {
        DayOfWeek.MONDAY -> "월요일"
        DayOfWeek.TUESDAY -> "화요일"
        DayOfWeek.WEDNESDAY -> "수요일"
        DayOfWeek.THURSDAY -> "목요일"
        DayOfWeek.FRIDAY -> "금요일"
        DayOfWeek.SATURDAY -> "토요일"
        DayOfWeek.SUNDAY -> "일요일"
    }

    /** "2026년 11월 1일 일요일" */
    fun dateLong(d: LocalDate): String = "${d.year}년 ${d.monthValue}월 ${d.dayOfMonth}일 ${weekday(d.dayOfWeek)}"

    /** "2026년 11월 1일" */
    fun dateMedium(d: LocalDate): String = "${d.year}년 ${d.monthValue}월 ${d.dayOfMonth}일"

    fun dayTitle(day: Long): String = dateLong(LocalDate.ofEpochDay(day))

    /** "오후 3:04" */
    fun time(t: LocalDateTime): String {
        val h = t.hour
        val (ampm, h12) = when {
            h == 0 -> "오전" to 12
            h < 12 -> "오전" to h
            h == 12 -> "오후" to 12
            else -> "오후" to h - 12
        }
        return "$ampm $h12:%02d".format(t.minute)
    }

    fun dateTimeKo(capturedMs: Long): String {
        val t = dateTime(capturedMs)
        return "${dateLong(t.toLocalDate())} ${time(t)}"
    }

    fun dateRange(firstMs: Long?, lastMs: Long?): String {
        if (firstMs == null || lastMs == null) return ""
        val a = dateTime(firstMs).toLocalDate()
        val b = dateTime(lastMs).toLocalDate()
        return if (a == b) dateMedium(a) else "${dateMedium(a)} – ${dateMedium(b)}"
    }

    /** "1,234" */
    fun count(n: Int): String = "%,d".format(Locale.US, n)

    /** "12.3 GB" (powers of 1000, like the desktop app). */
    fun bytes(n: Long): String {
        val units = arrayOf("B", "KB", "MB", "GB", "TB")
        var v = n.toDouble()
        var i = 0
        while (v >= 1000.0 && i < units.size - 1) {
            v /= 1000.0
            i++
        }
        return when {
            i == 0 -> "$n B"
            v >= 100.0 -> "%.0f %s".format(Locale.US, v, units[i])
            else -> "%.1f %s".format(Locale.US, v, units[i])
        }
    }
}
