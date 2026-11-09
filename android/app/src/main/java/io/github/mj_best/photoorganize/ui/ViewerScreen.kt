package io.github.mj_best.photoorganize.ui

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.focusable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.calculatePan
import androidx.compose.foundation.gestures.calculateZoom
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.pager.HorizontalPager
import androidx.compose.foundation.pager.rememberPagerState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.ArrowBack
import androidx.compose.material.icons.outlined.Block
import androidx.compose.material.icons.outlined.Delete
import androidx.compose.material.icons.outlined.Flag
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.mj_best.photoorganize.core.Photo
import io.github.mj_best.photoorganize.data.rememberPreview
import io.github.mj_best.photoorganize.data.rememberThumbnail
import kotlinx.coroutines.launch
import kotlin.math.abs

/** What the photo view does. */
interface ViewerActions {
    fun close()
    fun moved(index: Int)
    fun rate(id: Long, rating: Int)
    fun flag(id: Long, flag: Int)
    fun delete(id: Long)
}

@Composable
fun ViewerScreen(
    state: ViewerState,
    photo: (Long) -> Photo?,
    threshold: Int,
    act: ViewerActions,
    snackbar: SnackbarHostState,
) {
    val p = LocalPalette.current
    val pager = rememberPagerState(initialPage = state.index) { state.ids.size }
    val scope = rememberCoroutineScope()
    var zoomed by remember { mutableStateOf(false) }
    val focus = remember { FocusRequester() }
    LaunchedEffect(pager) { snapshotFlow { pager.currentPage }.collect { act.moved(it) } }
    LaunchedEffect(Unit) { focus.requestFocus() }
    val id = state.ids.getOrNull(pager.currentPage) ?: return
    val current = photo(id)

    fun go(delta: Int) {
        val to = (pager.currentPage + delta).coerceIn(0, state.ids.size - 1)
        scope.launch { pager.animateScrollToPage(to) }
    }

    // A keyboard on a tablet: the desktop shortcuts.
    val keys = Modifier.onPreviewKeyEvent { e ->
        if (e.type != KeyEventType.KeyDown) return@onPreviewKeyEvent false
        when (e.key) {
            Key.One, Key.F1 -> act.rate(id, 1)
            Key.Two, Key.F2 -> act.rate(id, 2)
            Key.Three, Key.F3 -> act.rate(id, 3)
            Key.Four, Key.F4 -> act.rate(id, 4)
            Key.Five, Key.F5 -> act.rate(id, 5)
            Key.Zero, Key.F6 -> act.rate(id, 0)
            Key.P -> act.flag(id, if ((current?.flag ?: 0) > 0) 0 else 1)
            Key.X -> act.flag(id, if ((current?.flag ?: 0) < 0) 0 else -1)
            Key.U -> act.flag(id, 0)
            Key.DirectionLeft -> go(-1)
            Key.DirectionRight, Key.Spacebar -> go(1)
            Key.Delete, Key.Backspace -> act.delete(id)
            Key.Escape -> act.close()
            else -> return@onPreviewKeyEvent false
        }
        true
    }

    BoxWithConstraints(Modifier.fillMaxSize().background(p.viewer).then(keys).focusRequester(focus).focusable()) {
        val wide = maxWidth >= 840.dp && maxWidth > maxHeight
        val edge = with(LocalDensity.current) { maxOf(maxWidth, maxHeight).toPx() }.toInt().coerceIn(1024, 3200)
        Row(Modifier.fillMaxSize()) {
            Box(Modifier.weight(1f).fillMaxHeight()) {
                HorizontalPager(
                    state = pager,
                    key = { state.ids[it] },
                    userScrollEnabled = !zoomed,
                    beyondViewportPageCount = 1,
                    modifier = Modifier.fillMaxSize(),
                ) { page ->
                    val pid = state.ids[page]
                    ZoomableImage(
                        id = pid,
                        edge = edge,
                        onZoom = { if (page == pager.currentPage) zoomed = it },
                        onSwipeUp = { act.flag(pid, 1) },
                        onSwipeDown = { act.flag(pid, -1) },
                    )
                }
                TopRow(current, act, Modifier.align(Alignment.TopStart))
                FlagToast(current, Modifier.align(Alignment.Center))
                if (!wide) {
                    Column(Modifier.align(Alignment.BottomCenter)) {
                        SnackbarHost(snackbar)
                        InfoPanel(current, threshold, act, Modifier.fillMaxWidth().navigationBarsPadding())
                    }
                } else {
                    SnackbarHost(snackbar, Modifier.align(Alignment.BottomCenter))
                }
            }
            if (wide) {
                Box(Modifier.width(1.dp).fillMaxHeight().background(p.separator))
                InfoPanel(
                    current, threshold, act,
                    Modifier.width(340.dp).fillMaxHeight().verticalScroll(rememberScrollState()).statusBarsPadding(),
                )
            }
        }
    }
}

@Composable
private fun TopRow(photo: Photo?, act: ViewerActions, modifier: Modifier) {
    Row(
        modifier
            .fillMaxWidth()
            .background(androidx.compose.ui.graphics.Brush.verticalGradient(listOf(Color(0x88000000), Color.Transparent)))
            .statusBarsPadding()
            .padding(horizontal = 4.dp, vertical = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        IconButton(onClick = act::close) { Icon(Icons.AutoMirrored.Outlined.ArrowBack, "닫기", tint = Color.White) }
        Column(Modifier.weight(1f)) {
            Text(photo?.name ?: "", color = Color.White, fontWeight = FontWeight.SemiBold, fontSize = 16.sp)
            if (photo != null) Text(Fmt.dateTimeKo(photo.capturedMs), color = Color(0xDDFFFFFF), fontSize = 12.sp)
        }
        if (photo != null) {
            IconButton(onClick = { act.delete(photo.id) }) { Icon(Icons.Outlined.Delete, "휴지통으로", tint = Color.White) }
        }
    }
}

/** Shows for a moment what was just decided. */
@Composable
private fun FlagToast(photo: Photo?, modifier: Modifier) {
    val p = LocalPalette.current
    var last by remember { mutableStateOf<Pair<Long, Int>?>(null) }
    var show by remember { mutableStateOf(false) }
    val now = photo?.let { it.id to it.flag }
    LaunchedEffect(now) {
        val before = last
        last = now
        if (before != null && now != null && before.first == now.first && before.second != now.second) {
            show = true
            kotlinx.coroutines.delay(700)
            show = false
        }
    }
    AnimatedVisibility(show, modifier, enter = fadeIn(), exit = fadeOut()) {
        val (text, color) = when ((photo?.flag ?: 0)) {
            1 -> "픽 — A컷" to p.success
            -1 -> "거절 — B컷" to p.danger
            else -> "픽/거절 해제" to p.text2
        }
        Text(
            text, color = Color.White, fontSize = 18.sp, fontWeight = FontWeight.SemiBold,
            modifier = Modifier.clip(RoundedCornerShape(12.dp)).background(color.copy(alpha = 0.9f))
                .padding(horizontal = 18.dp, vertical = 10.dp),
        )
    }
}

@Composable
private fun ZoomableImage(
    id: Long,
    edge: Int,
    onZoom: (Boolean) -> Unit,
    onSwipeUp: () -> Unit,
    onSwipeDown: () -> Unit,
) {
    val thumb = rememberThumbnail(id)
    val big = rememberPreview(id, edge)
    var scale by remember { mutableFloatStateOf(1f) }
    var offset by remember { mutableStateOf(Offset.Zero) }
    val swipe = with(LocalDensity.current) { 90.dp.toPx() }
    Box(
        Modifier
            .fillMaxSize()
            .pointerInput(id) {
                detectTapGestures(onDoubleTap = { at ->
                    if (scale > 1f) {
                        scale = 1f
                        offset = Offset.Zero
                    } else {
                        scale = 2.5f
                        offset = (Offset(size.width / 2f, size.height / 2f) - at) * 1.5f
                    }
                    onZoom(scale > 1f)
                })
            }
            .pointerInput(id) {
                awaitEachGesture {
                    awaitFirstDown(requireUnconsumed = false)
                    var total = Offset.Zero
                    var multi = false
                    do {
                        val event = awaitPointerEvent()
                        val pressed = event.changes.count { it.pressed }
                        if (pressed > 1 || scale > 1f) {
                            multi = multi || pressed > 1
                            scale = (scale * event.calculateZoom()).coerceIn(1f, 6f)
                            offset = if (scale == 1f) Offset.Zero else offset + event.calculatePan()
                            event.changes.forEach { it.consume() }
                            onZoom(scale > 1f)
                        } else {
                            total += event.calculatePan()
                        }
                    } while (event.changes.any { it.pressed })
                    // A vertical swipe on the whole photo: up picks, down rejects.
                    if (!multi && scale == 1f && abs(total.y) > swipe && abs(total.y) > 2 * abs(total.x)) {
                        if (total.y < 0) onSwipeUp() else onSwipeDown()
                    }
                }
            },
        contentAlignment = Alignment.Center,
    ) {
        val img = big ?: thumb
        if (img != null) {
            Image(
                img, null,
                Modifier.fillMaxSize().graphicsLayer {
                    scaleX = scale
                    scaleY = scale
                    translationX = offset.x
                    translationY = offset.y
                },
                contentScale = ContentScale.Fit,
            )
        }
    }
}

@OptIn(ExperimentalLayoutApi::class, ExperimentalFoundationApi::class)
@Composable
fun InfoPanel(photo: Photo?, threshold: Int, act: ViewerActions, modifier: Modifier) {
    val p = LocalPalette.current
    Column(
        modifier.background(p.panel.copy(alpha = 0.97f)).padding(horizontal = 18.dp, vertical = 12.dp),
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        if (photo == null) return@Column
        // Stars.
        Row(verticalAlignment = Alignment.CenterVertically) {
            for (r in 1..5) {
                Text(
                    if (r <= photo.rating) "★" else "☆",
                    color = if (r <= photo.rating) p.star else p.text3,
                    fontSize = 30.sp,
                    modifier = Modifier
                        .clip(RoundedCornerShape(6.dp))
                        .clickable { act.rate(photo.id, if (photo.rating == r) 0 else r) }
                        .padding(horizontal = 4.dp),
                )
            }
            Spacer(Modifier.weight(1f))
        }
        // Pick / reject.
        Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
            FlagButton("픽", Icons.Outlined.Flag, p.success, photo.flag > 0, Modifier.weight(1f)) {
                act.flag(photo.id, if (photo.flag > 0) 0 else 1)
            }
            FlagButton("거절", Icons.Outlined.Block, p.danger, photo.flag < 0, Modifier.weight(1f)) {
                act.flag(photo.id, if (photo.flag < 0) 0 else -1)
            }
        }
        // The culling assistant.
        val b = photo.bPct
        if (b != null) {
            Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text("B컷 가능성", color = p.text2, fontSize = 13.sp, modifier = Modifier.weight(1f))
                    Text(
                        "$b%", fontWeight = FontWeight.SemiBold, fontSize = 15.sp,
                        color = if (b >= threshold) p.warning else p.success,
                    )
                }
                LinearProgressIndicator(
                    progress = { b / 100f },
                    modifier = Modifier.fillMaxWidth().height(5.dp).clip(RoundedCornerShape(3.dp)),
                    color = if (b >= threshold) p.warning else p.success,
                    trackColor = p.separator,
                    drawStopIndicator = {},
                )
                val notes = buildList {
                    photo.topPct?.let { if (it <= 30) add("가장 잘 나온 사진 상위 $it%") }
                    if (photo.burstSize > 0) add("연사 ${photo.burstSize}장 중 ${photo.burstRank}번째")
                    if (photo.scene.isNotEmpty()) add(photo.scene)
                }
                if (photo.reasons.isNotEmpty() || notes.isNotEmpty()) {
                    FlowRow(horizontalArrangement = Arrangement.spacedBy(6.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                        photo.reasons.forEach { Chip(it, p.warning) }
                        notes.forEach { Chip(it, p.text2) }
                    }
                }
            }
        } else {
            Text("셀렉 도우미가 아직 보지 않은 사진이에요.", color = p.text3, fontSize = 13.sp)
        }
        // Camera.
        val camera = listOf(photo.camera, photo.lens).filter { it.isNotBlank() }.joinToString(" · ")
        if (camera.isNotBlank() || photo.exposure.isNotBlank()) {
            Column {
                if (camera.isNotBlank()) Text(camera, color = p.text, fontSize = 13.sp)
                if (photo.exposure.isNotBlank()) Text(photo.exposure, color = p.text2, fontSize = 13.sp)
                val kind = listOfNotNull(if (photo.raw) "RAW" else null, if (photo.jpeg) "JPEG" else null).joinToString("+")
                Text(
                    listOf(kind, if (photo.width > 0) "${photo.width} × ${photo.height}" else "").filter { it.isNotEmpty() }.joinToString(" · "),
                    color = p.text3, fontSize = 12.sp,
                )
            }
        }
        Text("위로 밀면 픽, 아래로 밀면 거절 · 두 번 눌러 확대", color = p.text3, fontSize = 11.sp)
    }
}

@Composable
private fun Chip(text: String, color: Color) {
    Text(
        text, color = color, fontSize = 12.sp,
        modifier = Modifier.clip(RoundedCornerShape(10.dp)).background(color.copy(alpha = 0.12f))
            .padding(horizontal = 9.dp, vertical = 3.dp),
    )
}

@Composable
private fun FlagButton(
    label: String,
    icon: androidx.compose.ui.graphics.vector.ImageVector,
    color: Color,
    on: Boolean,
    modifier: Modifier,
    onClick: () -> Unit,
) {
    val p = LocalPalette.current
    Row(
        modifier
            .clip(RoundedCornerShape(10.dp))
            .background(if (on) color else p.control)
            .clickable(onClick = onClick)
            .padding(vertical = 11.dp),
        horizontalArrangement = Arrangement.Center,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(icon, null, tint = if (on) Color.White else color, modifier = Modifier.size(18.dp))
        Spacer(Modifier.width(6.dp))
        Text(label, color = if (on) Color.White else p.text, fontWeight = FontWeight.SemiBold)
    }
}
