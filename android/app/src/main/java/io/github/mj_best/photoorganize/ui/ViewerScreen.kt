package io.github.mj_best.photoorganize.ui

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.animateContentSize
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.foundation.Canvas
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
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.pager.HorizontalPager
import androidx.compose.foundation.pager.rememberPagerState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.ArrowBack
import androidx.compose.material.icons.outlined.Block
import androidx.compose.material.icons.outlined.CropRotate
import androidx.compose.material.icons.outlined.Delete
import androidx.compose.material.icons.outlined.ExpandLess
import androidx.compose.material.icons.outlined.ExpandMore
import androidx.compose.material.icons.outlined.Flag
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.lerp
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.mj_best.photoorganize.core.Photo
import io.github.mj_best.photoorganize.data.rememberPreview
import io.github.mj_best.photoorganize.data.rememberThumbnail
import kotlinx.coroutines.launch
import kotlin.math.abs
import kotlin.math.max
import kotlin.math.min

/** What the photo view does. */
interface ViewerActions {
    fun close()
    fun moved(index: Int)
    fun rate(id: Long, rating: Int)
    fun flag(id: Long, flag: Int)
    fun delete(id: Long)
    fun autoFrame(id: Long, compose: Boolean) {}
    fun resetFraming(id: Long) {}

    /** Opens or folds the details of the panel under the photo (phone, 7" tablet). */
    fun showDetails(on: Boolean) {}
}

/**
 * The photo view. [details]: on a phone or 7" tablet, whether the panel under the photo shows
 * everything (reasons, composition, framing, camera) or only stars, pick / reject and the gauge.
 */
@Composable
fun ViewerScreen(
    state: ViewerState,
    photo: (Long) -> Photo?,
    threshold: Int,
    act: ViewerActions,
    snackbar: SnackbarHostState,
    details: Boolean = false,
) {
    val p = LocalPalette.current
    val pager = rememberPagerState(initialPage = state.index) { state.ids.size }
    val scope = rememberCoroutineScope()
    // Zoom state is kept across photos so bursts can be compared while zoomed in.
    var scale by remember { mutableFloatStateOf(1f) }
    var offset by remember { mutableStateOf(Offset.Zero) }
    var lastSpaceMs by remember { mutableLongStateOf(0L) }
    val zoomed = scale > 1f
    val focus = remember { FocusRequester() }
    LaunchedEffect(pager) { snapshotFlow { pager.currentPage }.collect { act.moved(it) } }
    LaunchedEffect(Unit) { focus.requestFocus() }
    val id = state.ids.getOrNull(pager.currentPage) ?: return
    val current = photo(id)

    fun go(delta: Int) {
        val to = (pager.currentPage + delta).coerceIn(0, state.ids.size - 1)
        scope.launch { pager.animateScrollToPage(to) }
    }

    fun toggleCenterZoom() {
        if (scale > 1f) {
            scale = 1f
            offset = Offset.Zero
        } else {
            scale = 2.5f
            offset = Offset.Zero
        }
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
            Key.Z -> toggleCenterZoom()
            Key.R -> act.autoFrame(id, false)
            Key.I -> act.showDetails(!details)
            Key.DirectionLeft -> go(-1)
            Key.DirectionRight -> go(1)
            Key.Spacebar -> {
                val now = System.currentTimeMillis()
                if (now - lastSpaceMs <= 300L) {
                    lastSpaceMs = 0L
                    toggleCenterZoom()
                } else {
                    lastSpaceMs = now
                    go(1)
                }
            }
            Key.Delete, Key.Backspace -> act.delete(id)
            Key.Escape -> act.close()
            else -> return@onPreviewKeyEvent false
        }
        true
    }

    BoxWithConstraints(Modifier.fillMaxSize().background(p.viewer).then(keys).focusRequester(focus).focusable()) {
        val wide = maxWidth >= 840.dp && maxWidth > maxHeight
        val edge = with(LocalDensity.current) { maxOf(maxWidth, maxHeight).toPx() }.toInt().coerceIn(1024, 3200)
        // Phone / 7": the most the panel under the photo may take (navigation bar included).
        val panelMax = maxHeight * 0.5f
        // The photo (with its top row over it on a 10" tablet); clipped so a zoomed photo never
        // draws over the panel or the top row.
        val photoArea: @Composable (Modifier) -> Unit = { modifier ->
            Box(modifier.clipToBounds().testTag(PHOTO_TAG)) {
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
                        scale = scale,
                        offset = offset,
                        onTransform = { s, o ->
                            scale = s
                            offset = o
                        },
                        onSwipeUp = { act.flag(pid, 1) },
                        onSwipeDown = { act.flag(pid, -1) },
                    )
                }
                if (wide) {
                    TopRow(current, act, Modifier.align(Alignment.TopStart), overlay = true)
                    Column(
                        Modifier.align(Alignment.BottomCenter).padding(bottom = 12.dp),
                        horizontalAlignment = Alignment.CenterHorizontally,
                    ) {
                        FlagToast(current, Modifier.padding(bottom = 6.dp))
                        SnackbarHost(snackbar)
                    }
                }
            }
        }
        if (wide) {
            // 10" landscape: the panel beside the photo.
            Row(Modifier.fillMaxSize()) {
                photoArea(Modifier.weight(1f).fillMaxHeight())
                Box(Modifier.width(1.dp).fillMaxHeight().background(p.separator))
                InfoPanel(
                    current, threshold, act,
                    Modifier.width(340.dp).fillMaxHeight().verticalScroll(rememberScrollState()).statusBarsPadding()
                        .testTag(PANEL_TAG),
                )
            }
        } else {
            // Phone and 7" tablet: the photo takes the space between the top row and the panel,
            // never under either (DEVLOG 3절 "사진을 가리지 말 것"). The panel is compact unless the
            // details are open, and even then takes at most half the height; the details scroll
            // inside it.
            Column(Modifier.fillMaxSize()) {
                TopRow(current, act, Modifier, overlay = false)
                photoArea(Modifier.weight(1f).fillMaxWidth())
                Box(Modifier.fillMaxWidth().height(1.dp).background(p.separator))
                Box(Modifier.fillMaxWidth()) {
                    BottomPanel(
                        current, threshold, act, details,
                        maxHeight = panelMax,
                        modifier = Modifier.fillMaxWidth().testTag(PANEL_TAG),
                    )
                    // Notices go over the panel's bottom (the camera lines when open), not over the photo.
                    Column(
                        Modifier.align(Alignment.BottomCenter).navigationBarsPadding().padding(bottom = 6.dp),
                        horizontalAlignment = Alignment.CenterHorizontally,
                    ) {
                        FlagToast(current, Modifier.padding(bottom = 6.dp))
                        SnackbarHost(snackbar)
                    }
                }
            }
        }
    }
}

/** Test tags: the photo area, the info panel and the top row (phone, 7"), which must never overlap. */
const val PHOTO_TAG = "viewer-photo"
const val PANEL_TAG = "viewer-panel"
const val TOP_TAG = "viewer-top"

/** Back, the name and date, delete: over the photo's top edge ([overlay]), or a bar above it. */
@Composable
private fun TopRow(photo: Photo?, act: ViewerActions, modifier: Modifier, overlay: Boolean) {
    val p = LocalPalette.current
    val fg = if (overlay) Color.White else p.text
    Row(
        modifier
            .testTag(TOP_TAG)
            .fillMaxWidth()
            .background(
                if (overlay) {
                    Brush.verticalGradient(listOf(Color(0x88000000), Color.Transparent))
                } else {
                    SolidColor(p.toolbar)
                },
            )
            .statusBarsPadding()
            .padding(horizontal = 4.dp, vertical = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        IconButton(onClick = act::close) { Icon(Icons.AutoMirrored.Outlined.ArrowBack, "닫기", tint = fg) }
        Column(Modifier.weight(1f)) {
            Text(
                photo?.name ?: "",
                color = fg,
                fontWeight = FontWeight.SemiBold,
                fontSize = 16.sp,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            if (photo != null) {
                Text(
                    Fmt.dateTimeKo(photo.capturedMs),
                    color = if (overlay) Color(0xDDFFFFFF) else p.text2,
                    fontSize = 12.sp,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
        }
        if (photo != null) {
            IconButton(onClick = { act.delete(photo.id) }) { Icon(Icons.Outlined.Delete, "휴지통으로", tint = fg) }
        }
    }
}

/** Shows for a moment at the bottom strip what was just decided (never over the center of the photo). */
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
            text, color = Color.White, fontSize = 14.sp, fontWeight = FontWeight.SemiBold,
            modifier = Modifier.clip(RoundedCornerShape(16.dp)).background(color.copy(alpha = 0.92f))
                .padding(horizontal = 16.dp, vertical = 7.dp),
        )
    }
}

@Composable
private fun ZoomableImage(
    id: Long,
    edge: Int,
    scale: Float,
    offset: Offset,
    onTransform: (Float, Offset) -> Unit,
    onSwipeUp: () -> Unit,
    onSwipeDown: () -> Unit,
) {
    val thumb = rememberThumbnail(id)
    val big = rememberPreview(id, edge)
    val swipe = with(LocalDensity.current) { 90.dp.toPx() }
    var curScale = scale
    var curOffset = offset
    Box(
        Modifier
            .fillMaxSize()
            .pointerInput(id) {
                detectTapGestures(onDoubleTap = { at ->
                    if (curScale > 1f) {
                        onTransform(1f, Offset.Zero)
                    } else {
                        onTransform(2.5f, (Offset(size.width / 2f, size.height / 2f) - at) * 1.5f)
                    }
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
                        if (pressed > 1 || curScale > 1f) {
                            multi = multi || pressed > 1
                            val nextScale = (curScale * event.calculateZoom()).coerceIn(1f, 6f)
                            val nextOffset = if (nextScale == 1f) Offset.Zero else curOffset + event.calculatePan()
                            curScale = nextScale
                            curOffset = nextOffset
                            onTransform(nextScale, nextOffset)
                            event.changes.forEach { it.consume() }
                        } else {
                            total += event.calculatePan()
                        }
                    } while (event.changes.any { it.pressed })
                    // A vertical swipe on the whole photo: up picks, down rejects.
                    if (!multi && curScale == 1f && abs(total.y) > swipe && abs(total.y) > 2 * abs(total.x)) {
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

/** 10" landscape: everything, beside the photo. */
@Composable
fun InfoPanel(photo: Photo?, threshold: Int, act: ViewerActions, modifier: Modifier) {
    val p = LocalPalette.current
    Column(
        modifier.background(p.panel).padding(horizontal = 18.dp, vertical = 12.dp),
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        if (photo == null) return@Column
        PanelHead(photo, threshold, act, toggle = null)
        PanelDetails(photo, act)
    }
}

/**
 * Phone and 7" tablet: under the photo. Folded, only what every photo needs (stars, pick / reject,
 * the gauge); open, the details too, scrolling under a head that stays put. At most [maxHeight].
 */
@Composable
private fun BottomPanel(
    photo: Photo?,
    threshold: Int,
    act: ViewerActions,
    details: Boolean,
    maxHeight: Dp,
    modifier: Modifier,
) {
    val p = LocalPalette.current
    Column(
        modifier
            .background(p.panel)
            .animateContentSize()
            .heightIn(max = maxHeight)
            .navigationBarsPadding()
            .padding(horizontal = 18.dp, vertical = 10.dp),
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        if (photo == null) return@Column
        PanelHead(photo, threshold, act) { DetailsToggle(details) { act.showDetails(!details) } }
        if (details) {
            PanelDetails(photo, act, Modifier.weight(1f, fill = false).verticalScroll(rememberScrollState()))
        }
    }
}

/** "자세히 ⌃" / "접기 ⌄" at the end of the stars row. */
@Composable
private fun DetailsToggle(open: Boolean, onClick: () -> Unit) {
    val p = LocalPalette.current
    Row(
        Modifier
            .heightIn(min = 40.dp)
            .clip(RoundedCornerShape(8.dp))
            .clickable(onClick = onClick)
            .padding(start = 10.dp, end = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(if (open) "접기" else "자세히", color = p.text2, fontSize = 13.sp, fontWeight = FontWeight.Medium)
        Icon(
            if (open) Icons.Outlined.ExpandMore else Icons.Outlined.ExpandLess, null,
            tint = p.text2, modifier = Modifier.size(20.dp),
        )
    }
}

/** Stars, pick / reject and the assistant's gauge: what culling needs on every photo. */
@Composable
private fun PanelHead(photo: Photo, threshold: Int, act: ViewerActions, toggle: (@Composable () -> Unit)?) {
    val p = LocalPalette.current
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
        toggle?.invoke()
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
    val b = photo.bPct
    if (b != null) {
        Gauge(b, threshold)
    } else {
        Text("셀렉 도우미가 아직 보지 않은 사진이에요.", color = p.text3, fontSize = 13.sp)
    }
}

/** The culling assistant: left = B-cut, right = A-cut. */
@Composable
private fun Gauge(b: Int, threshold: Int) {
    val p = LocalPalette.current
    val bPct = b.coerceIn(0, 100)
    val aPct = 100 - bPct
    val isB = bPct >= threshold
    val isA = !isB && bPct < 30
    val gaugeColor = when {
        isB -> p.warning
        isA -> p.success
        else -> p.text2
    }
    Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(
                "← B컷 $bPct%",
                color = when {
                    isB -> p.warning
                    !isA && bPct > aPct -> p.text
                    bPct == aPct -> p.text2
                    else -> p.text3
                },
                fontWeight = if (isB || (!isA && bPct > aPct)) FontWeight.SemiBold else FontWeight.Normal,
                fontSize = if (isB) 14.sp else 12.sp,
                modifier = Modifier.weight(1f),
            )
            Text(
                "A컷 $aPct% →",
                color = when {
                    isA -> p.success
                    !isB && aPct > bPct -> p.text
                    bPct == aPct -> p.text2
                    else -> p.text3
                },
                fontWeight = if (isA || (!isB && aPct > bPct)) FontWeight.SemiBold else FontWeight.Normal,
                fontSize = if (isA) 14.sp else 12.sp,
            )
        }
        Canvas(modifier = Modifier.fillMaxWidth().height(16.dp)) {
            val pad = 7.dp.toPx()
            val trackH = 6.dp.toPx()
            val r = trackH / 2f
            val cy = size.height / 2f
            val left = min(pad, size.width / 2f)
            val right = max(size.width - pad, size.width / 2f)
            val trackW = (right - left).coerceAtLeast(0f)
            val leftTint = lerp(p.control, p.warning, 0.36f)
            val rightTint = lerp(p.control, p.success, 0.36f)
            drawRoundRect(
                brush = Brush.horizontalGradient(
                    colors = listOf(leftTint, p.control, rightTint),
                    startX = left,
                    endX = right,
                ),
                topLeft = Offset(left, cy - r),
                size = Size(trackW, trackH),
                cornerRadius = CornerRadius(r, r),
            )
            val cutX = left + trackW * (1f - threshold.coerceIn(0, 100) / 100f)
            val posX = left + trackW * (aPct / 100f)
            val segW = abs(posX - cutX)
            if (segW > 0.5f) {
                drawRoundRect(
                    color = gaugeColor,
                    topLeft = Offset(min(cutX, posX), cy - r),
                    size = Size(segW, trackH),
                    cornerRadius = CornerRadius(r, r),
                )
            }
            drawLine(
                color = p.text3,
                start = Offset(cutX, cy - r - 3.dp.toPx()),
                end = Offset(cutX, cy + r + 3.dp.toPx()),
                strokeWidth = 1.5.dp.toPx(),
            )
            drawCircle(color = p.panel, radius = 6.5.dp.toPx(), center = Offset(posX, cy))
            drawCircle(color = gaugeColor, radius = 4.8.dp.toPx(), center = Offset(posX, cy))
        }
    }
}

/** Why the assistant thinks so (reasons, composition), the framing tools and the camera. */
@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun PanelDetails(photo: Photo, act: ViewerActions, modifier: Modifier = Modifier) {
    val p = LocalPalette.current
    Column(modifier, verticalArrangement = Arrangement.spacedBy(10.dp)) {
        if (photo.bPct != null) {
            Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
                val notes = buildList {
                    photo.topPct?.let { if (it <= 30) add("가장 잘 나온 사진 상위 $it%") }
                    if (photo.burstSize > 0) add("연사 ${photo.burstSize}장 중 ${photo.burstRank}번째")
                    if (photo.scene.isNotEmpty()) add(photo.scene)
                }
                if (photo.reasons.isNotEmpty() || photo.strengths.isNotEmpty() || notes.isNotEmpty()) {
                    FlowRow(horizontalArrangement = Arrangement.spacedBy(6.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                        photo.reasons.forEach { Chip(it, p.warning) }
                        photo.strengths.forEach { Chip(it, p.success) }
                        notes.forEach { Chip(it, p.text2) }
                    }
                }
                if (photo.compNotes.isNotEmpty()) {
                    Column(verticalArrangement = Arrangement.spacedBy(3.dp)) {
                        photo.compNotes.forEach { n ->
                            val dot = when {
                                n.ignored || n.ok == null -> p.text3
                                n.ok == true -> p.success
                                else -> p.warning
                            }
                            Row(verticalAlignment = Alignment.CenterVertically) {
                                Box(Modifier.size(7.dp).clip(CircleShape).background(dot))
                                Spacer(Modifier.width(7.dp))
                                Text(
                                    if (n.ignored) "${n.text} (꺼 둠)" else n.text,
                                    color = if (n.ignored) p.text3 else p.text2,
                                    fontSize = 12.sp,
                                )
                            }
                        }
                    }
                }
                if (!photo.video) {
                    val framed = photo.framing
                    if (framed != null) {
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            Icon(Icons.Outlined.CropRotate, null, tint = p.accent, modifier = Modifier.size(15.dp))
                            Spacer(Modifier.width(5.dp))
                            Text("크롭 · 수평: $framed", color = p.accent, fontSize = 12.sp, modifier = Modifier.weight(1f))
                            Text(
                                "원래대로",
                                color = p.text2,
                                fontSize = 12.sp,
                                fontWeight = FontWeight.SemiBold,
                                modifier = Modifier
                                    .clip(RoundedCornerShape(6.dp))
                                    .clickable { act.resetFraming(photo.id) }
                                    .padding(horizontal = 6.dp, vertical = 2.dp),
                            )
                        }
                    }
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        ActionChip("자동 수평") { act.autoFrame(photo.id, false) }
                        ActionChip("자동 구도") { act.autoFrame(photo.id, true) }
                    }
                }
            }
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
private fun ActionChip(label: String, onClick: () -> Unit) {
    val p = LocalPalette.current
    Text(
        label,
        color = p.text,
        fontSize = 12.sp,
        fontWeight = FontWeight.Medium,
        modifier = Modifier
            .clip(RoundedCornerShape(8.dp))
            .background(p.control)
            .clickable(onClick = onClick)
            .padding(horizontal = 10.dp, vertical = 5.dp),
    )
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
