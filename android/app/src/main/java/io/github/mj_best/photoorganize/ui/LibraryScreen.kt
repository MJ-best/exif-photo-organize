package io.github.mj_best.photoorganize.ui

import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.GridItemSpan
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.ManageSearch
import androidx.compose.material.icons.automirrored.outlined.Undo
import androidx.compose.material.icons.outlined.AutoAwesome
import androidx.compose.material.icons.outlined.Block
import androidx.compose.material.icons.outlined.CheckCircle
import androidx.compose.material.icons.outlined.Close
import androidx.compose.material.icons.outlined.Delete
import androidx.compose.material.icons.outlined.DoneAll
import androidx.compose.material.icons.outlined.Download
import androidx.compose.material.icons.outlined.Flag
import androidx.compose.material.icons.outlined.PhotoLibrary
import androidx.compose.material.icons.outlined.PlayCircle
import androidx.compose.material.icons.outlined.RadioButtonUnchecked
import androidx.compose.material.icons.outlined.Settings
import androidx.compose.material.icons.outlined.Star
import androidx.compose.material.icons.outlined.StarOutline
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.FilterChip
import androidx.compose.material3.FilterChipDefaults
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.mj_best.photoorganize.R
import io.github.mj_best.photoorganize.core.Counts
import io.github.mj_best.photoorganize.core.Photo
import io.github.mj_best.photoorganize.core.View
import io.github.mj_best.photoorganize.data.rememberThumbnail

fun View.title(): String = when (this) {
    View.ALL -> "모든 사진"
    View.UNDECIDED -> "아직 고르지 않은 사진"
    View.RATED -> "별점 있는 사진"
    View.CANDIDATES -> "B컷 후보"
    View.KEEPERS -> "A컷 후보"
    View.REJECTED -> "거절 (B컷)"
    View.PICKED -> "픽 (A컷)"
}

fun View.icon(): ImageVector = when (this) {
    View.ALL -> Icons.Outlined.PhotoLibrary
    View.UNDECIDED -> Icons.Outlined.RadioButtonUnchecked
    View.RATED -> Icons.Outlined.Star
    View.CANDIDATES -> Icons.AutoMirrored.Outlined.ManageSearch
    View.KEEPERS -> Icons.Outlined.AutoAwesome
    View.REJECTED -> Icons.Outlined.Block
    View.PICKED -> Icons.Outlined.Flag
}

fun Counts.of(v: View): Int = when (v) {
    View.ALL -> all
    View.UNDECIDED -> undecided
    View.RATED -> rated
    View.CANDIDATES -> candidates
    View.KEEPERS -> keepers
    View.REJECTED -> rejected
    View.PICKED -> picked
}

private val PhotoViews = listOf(View.ALL, View.UNDECIDED, View.RATED)
private val CullViews = listOf(View.CANDIDATES, View.KEEPERS, View.REJECTED, View.PICKED)

/** What the library screen does when touched. */
interface LibraryActions {
    fun show(view: View)
    fun open(id: Long)
    fun toggleSelect(id: Long)
    fun selectAll()
    fun clearSelection()
    fun rate(ids: Collection<Long>, rating: Int)
    fun flag(ids: Collection<Long>, flag: Int)
    fun delete(ids: Collection<Long>)
    fun import()
    fun settings()
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun LibraryScreen(ui: LibraryUi, selection: Set<Long>, act: LibraryActions, snackbar: SnackbarHostState) {
    val p = LocalPalette.current
    BoxWithConstraints(Modifier.fillMaxSize().background(p.content)) {
        val wide = maxWidth >= 840.dp
        Scaffold(
            containerColor = p.content,
            snackbarHost = { SnackbarHost(snackbar) },
            topBar = {
                if (selection.isNotEmpty()) {
                    SelectionBar(selection, act)
                } else {
                    TopAppBar(
                        colors = TopAppBarDefaults.topAppBarColors(containerColor = p.toolbar),
                        title = {
                            Column {
                                Text(ui.view.title(), fontWeight = FontWeight.SemiBold, fontSize = 19.sp)
                                Text(
                                    "${Fmt.count(ui.photos.size)}장",
                                    color = p.text2,
                                    fontSize = 12.sp,
                                )
                            }
                        },
                        actions = {
                            FilledTonalButton(onClick = act::import, modifier = Modifier.padding(end = 4.dp)) {
                                Icon(Icons.Outlined.Download, null, Modifier.size(18.dp))
                                Spacer(Modifier.width(6.dp))
                                Text("가져오기")
                            }
                            IconButton(onClick = act::settings) { Icon(Icons.Outlined.Settings, "설정") }
                        },
                    )
                }
            },
        ) { pad ->
            Row(Modifier.padding(pad).fillMaxSize()) {
                if (wide) {
                    SidePanel(ui, act, Modifier.width(260.dp).fillMaxHeight())
                    Box(Modifier.width(1.dp).fillMaxHeight().background(p.separator))
                }
                Column(Modifier.fillMaxSize()) {
                    val empty = ui.ready && ui.counts.all == 0
                    if (!wide && !empty) ViewChips(ui, act)
                    if (!empty) AssistantBar(ui, act)
                    when {
                        ui.error != null -> Message("라이브러리를 열지 못했습니다", ui.error)
                        !ui.ready -> Box(Modifier.fillMaxSize())
                        ui.counts.all == 0 -> Welcome(act)
                        ui.photos.isEmpty() -> Message(ui.view.title(), emptyText(ui.view))
                        else -> PhotoGrid(ui, selection, act, tile = if (wide) 150.dp else 86.dp)
                    }
                }
            }
        }
    }
}

private fun emptyText(v: View): String = when (v) {
    View.CANDIDATES -> "지금 기준으로는 B컷 후보가 없어요. 셀렉 도우미가 새 사진을 보면 여기에 모입니다."
    View.KEEPERS -> "셀렉 도우미가 사진을 보고 나면 가장 잘 나온 사진이 여기에 모입니다."
    View.REJECTED -> "거절한 사진이 없어요. 사진을 크게 보고 아래로 밀거나 X 를 누르면 거절됩니다."
    View.PICKED -> "픽한 사진이 없어요. 사진을 크게 보고 위로 밀거나 P 를 누르면 픽됩니다."
    View.RATED -> "별점을 준 사진이 없어요."
    View.UNDECIDED -> "모든 사진을 골랐어요."
    View.ALL -> ""
}

@Composable
private fun SidePanel(ui: LibraryUi, act: LibraryActions, modifier: Modifier) {
    val p = LocalPalette.current
    Column(modifier.background(p.sidebar).verticalScroll(rememberScrollState()).padding(vertical = 12.dp)) {
        SectionTitle("사진")
        PhotoViews.forEach { SideItem(it, ui, act) }
        Spacer(Modifier.height(10.dp))
        SectionTitle("셀렉 도우미")
        CullViews.forEach { SideItem(it, ui, act) }
    }
}

@Composable
private fun SectionTitle(text: String) {
    Text(
        text,
        color = LocalPalette.current.text3,
        fontSize = 12.sp,
        modifier = Modifier.padding(start = 22.dp, top = 6.dp, bottom = 4.dp),
    )
}

@Composable
private fun SideItem(v: View, ui: LibraryUi, act: LibraryActions) {
    val p = LocalPalette.current
    val sel = v == ui.view
    Row(
        Modifier
            .fillMaxWidth()
            .padding(horizontal = 10.dp, vertical = 1.dp)
            .clip(RoundedCornerShape(8.dp))
            .background(if (sel) p.selected else Color.Transparent)
            .clickable { act.show(v) }
            .padding(horizontal = 12.dp, vertical = 9.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(v.icon(), null, tint = p.accent, modifier = Modifier.size(19.dp))
        Spacer(Modifier.width(12.dp))
        Text(v.title(), color = p.text, fontSize = 14.sp, modifier = Modifier.weight(1f), maxLines = 1)
        Text(Fmt.count(ui.counts.of(v)), color = p.text3, fontSize = 13.sp)
    }
}

@Composable
private fun ViewChips(ui: LibraryUi, act: LibraryActions) {
    val p = LocalPalette.current
    LazyRow(
        Modifier.fillMaxWidth().background(p.toolbar),
        contentPadding = PaddingValues(horizontal = 12.dp, vertical = 6.dp),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        items(PhotoViews + CullViews) { v ->
            FilterChip(
                selected = v == ui.view,
                onClick = { act.show(v) },
                label = { Text("${v.title()} ${Fmt.count(ui.counts.of(v))}") },
                leadingIcon = { Icon(v.icon(), null, Modifier.size(16.dp)) },
                colors = FilterChipDefaults.filterChipColors(
                    selectedContainerColor = p.selected,
                    iconColor = p.accent,
                    selectedLeadingIconColor = p.accent,
                ),
            )
        }
    }
}

/** What the culling assistant is doing, and the actions of the 셀렉 도우미 views. */
@Composable
private fun AssistantBar(ui: LibraryUi, act: LibraryActions) {
    val p = LocalPalette.current
    val analysis = ui.analysis
    val tint = when (ui.view) {
        View.CANDIDATES -> p.warning.copy(alpha = 0.12f)
        View.REJECTED -> p.danger.copy(alpha = 0.10f)
        else -> p.panel
    }
    Column(Modifier.fillMaxWidth().background(tint)) {
        Row(
            Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            val text = when {
                analysis != null -> "셀렉 도우미가 사진을 보는 중 · ${Fmt.count(analysis.first)} / ${Fmt.count(analysis.second)}"
                ui.view == View.CANDIDATES -> "B컷일 가능성 ${ui.threshold}% 이상 · 아직 고르지 않은 사진만"
                ui.view == View.KEEPERS -> "가장 잘 나온 사진 · 연사마다 한 장"
                ui.view == View.REJECTED -> "거절은 표시일 뿐, 파일은 그대로예요 · 학습 기록은 남아요"
                else -> ui.learn?.summary ?: "기본 기준"
            }
            Icon(Icons.Outlined.AutoAwesome, null, tint = p.accent, modifier = Modifier.size(16.dp))
            Spacer(Modifier.width(8.dp))
            Text(text, color = p.text2, fontSize = 13.sp, modifier = Modifier.weight(1f), maxLines = 2)
            val ids = ui.photos.map { it.id }
            if (analysis == null && ids.isNotEmpty()) {
                when (ui.view) {
                    View.CANDIDATES -> TextButton(onClick = { act.flag(ids, -1) }) {
                        Text("${Fmt.count(ids.size)}장 모두 거절", color = p.danger)
                    }
                    View.KEEPERS -> TextButton(onClick = { act.flag(ids, 1) }) {
                        Text("${Fmt.count(ids.size)}장 모두 픽")
                    }
                    View.REJECTED -> TextButton(onClick = { act.delete(ids) }) {
                        Text("${Fmt.count(ids.size)}장 휴지통으로…", color = p.danger)
                    }
                    else -> {}
                }
            }
        }
        if (analysis != null) {
            LinearProgressIndicator(
                progress = { if (analysis.second == 0) 0f else analysis.first.toFloat() / analysis.second },
                modifier = Modifier.fillMaxWidth().height(2.dp),
                color = p.accent,
                trackColor = p.separator,
            )
        } else {
            HorizontalDivider(color = p.separator)
        }
    }
}

private sealed interface GridRow {
    data class Header(val day: Long, val count: Int) : GridRow
    data class Tile(val photo: Photo) : GridRow
}

private fun rows(photos: List<Photo>): List<GridRow> {
    val out = ArrayList<GridRow>(photos.size + 32)
    var i = 0
    while (i < photos.size) {
        val day = Fmt.day(photos[i].capturedMs)
        var j = i
        while (j < photos.size && Fmt.day(photos[j].capturedMs) == day) j++
        out += GridRow.Header(day, j - i)
        for (k in i until j) out += GridRow.Tile(photos[k])
        i = j
    }
    return out
}

@Composable
private fun PhotoGrid(ui: LibraryUi, selection: Set<Long>, act: LibraryActions, tile: androidx.compose.ui.unit.Dp) {
    val p = LocalPalette.current
    val rows = remember(ui.photos) { rows(ui.photos) }
    LazyVerticalGrid(
        columns = GridCells.Adaptive(tile),
        modifier = Modifier.fillMaxSize(),
        contentPadding = PaddingValues(start = 8.dp, end = 8.dp, bottom = 24.dp),
        horizontalArrangement = Arrangement.spacedBy(4.dp),
        verticalArrangement = Arrangement.spacedBy(4.dp),
    ) {
        items(
            rows,
            key = { r -> if (r is GridRow.Header) "d${r.day}" else (r as GridRow.Tile).photo.id },
            span = { r -> if (r is GridRow.Header) GridItemSpan(maxLineSpan) else GridItemSpan(1) },
            contentType = { r -> if (r is GridRow.Header) 0 else 1 },
        ) { r ->
            when (r) {
                is GridRow.Header -> Row(
                    Modifier.padding(start = 6.dp, top = 16.dp, bottom = 6.dp),
                    verticalAlignment = Alignment.Bottom,
                ) {
                    Text(Fmt.dayTitle(r.day), color = p.text, fontWeight = FontWeight.SemiBold, fontSize = 16.sp)
                    Spacer(Modifier.width(8.dp))
                    Text("${Fmt.count(r.count)}장", color = p.text3, fontSize = 13.sp)
                }
                is GridRow.Tile -> PhotoTile(r.photo, ui.view, ui.threshold, r.photo.id in selection, selection.isNotEmpty(), act)
            }
        }
    }
}

@OptIn(ExperimentalFoundationApi::class)
@Composable
fun PhotoTile(photo: Photo, view: View, threshold: Int, selected: Boolean, selecting: Boolean, act: LibraryActions) {
    val p = LocalPalette.current
    val bmp = rememberThumbnail(photo.id)
    val shape = RoundedCornerShape(6.dp)
    Box(
        Modifier
            .aspectRatio(1f)
            .clip(shape)
            .background(p.placeholder)
            .then(if (selected) Modifier.border(3.dp, p.accent, shape) else Modifier)
            .combinedClickable(
                onClick = { if (selecting) act.toggleSelect(photo.id) else act.open(photo.id) },
                onLongClick = { act.toggleSelect(photo.id) },
            ),
    ) {
        if (bmp != null) {
            Image(bmp, photo.name, Modifier.fillMaxSize(), contentScale = ContentScale.Crop)
        }
        if (photo.video) {
            Icon(Icons.Outlined.PlayCircle, null, tint = Color.White, modifier = Modifier.align(Alignment.Center).size(30.dp))
        }
        // Pick / reject.
        when {
            photo.flag > 0 -> Badge(Icons.Outlined.Flag, p.success, Modifier.align(Alignment.TopStart))
            photo.flag < 0 -> Badge(Icons.Outlined.Block, p.danger, Modifier.align(Alignment.TopStart))
        }
        // What the culling assistant thinks.
        val b = photo.bPct
        val top = photo.topPct
        val label = when {
            view == View.KEEPERS && top != null -> "상위 ${top}%"
            b != null && b >= threshold && photo.flag == 0 && photo.rating == 0 -> "B ${b}%"
            else -> null
        }
        if (label != null) {
            Text(
                label,
                color = Color.White,
                fontSize = 11.sp,
                fontWeight = FontWeight.SemiBold,
                modifier = Modifier
                    .align(Alignment.TopEnd)
                    .padding(5.dp)
                    .clip(RoundedCornerShape(5.dp))
                    .background(if (view == View.KEEPERS) p.success else p.warning)
                    .padding(horizontal = 5.dp, vertical = 1.dp),
            )
        }
        if (selected) {
            Icon(
                Icons.Outlined.CheckCircle, null, tint = Color.White,
                modifier = Modifier.align(Alignment.TopEnd).padding(5.dp).clip(CircleShape).background(p.accent),
            )
        }
        if (photo.rating > 0 || photo.burstSize > 0) {
            Row(
                Modifier
                    .align(Alignment.BottomCenter)
                    .fillMaxWidth()
                    .background(Brush.verticalGradient(listOf(Color.Transparent, Color(0x99000000))))
                    .padding(horizontal = 6.dp, vertical = 4.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text("★".repeat(photo.rating), color = p.star, fontSize = 12.sp, modifier = Modifier.weight(1f))
                if (photo.burstSize > 0) {
                    Text("연사 ${photo.burstRank}/${photo.burstSize}", color = Color.White, fontSize = 10.sp)
                }
            }
        }
    }
}

@Composable
private fun Badge(icon: ImageVector, color: Color, modifier: Modifier) {
    Icon(
        icon, null, tint = Color.White,
        modifier = modifier.padding(5.dp).size(22.dp).clip(CircleShape).background(color).padding(3.dp),
    )
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun SelectionBar(selection: Set<Long>, act: LibraryActions) {
    val p = LocalPalette.current
    var stars by remember { mutableStateOf(false) }
    TopAppBar(
        colors = TopAppBarDefaults.topAppBarColors(containerColor = p.selected),
        navigationIcon = { IconButton(onClick = act::clearSelection) { Icon(Icons.Outlined.Close, "선택 해제") } },
        title = { Text("${Fmt.count(selection.size)}장 선택", fontSize = 18.sp) },
        actions = {
            IconButton(onClick = act::selectAll) { Icon(Icons.Outlined.DoneAll, "모두 선택") }
            Box {
                IconButton(onClick = { stars = true }) { Icon(Icons.Outlined.StarOutline, "별점") }
                DropdownMenu(expanded = stars, onDismissRequest = { stars = false }) {
                    for (r in 5 downTo 0) {
                        DropdownMenuItem(
                            text = { Text(if (r == 0) "별점 지우기" else "★".repeat(r), color = if (r == 0) p.text else p.star) },
                            onClick = {
                                stars = false
                                act.rate(selection, r)
                            },
                        )
                    }
                }
            }
            IconButton(onClick = { act.flag(selection, 1) }) { Icon(Icons.Outlined.Flag, "픽", tint = p.success) }
            IconButton(onClick = { act.flag(selection, -1) }) { Icon(Icons.Outlined.Block, "거절", tint = p.danger) }
            IconButton(onClick = { act.flag(selection, 0) }) { Icon(Icons.AutoMirrored.Outlined.Undo, "픽/거절 해제") }
            IconButton(onClick = { act.delete(selection) }) { Icon(Icons.Outlined.Delete, "휴지통으로") }
        },
    )
}

@Composable
private fun Welcome(act: LibraryActions) {
    val p = LocalPalette.current
    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(24.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
    ) {
        Image(
            painterResource(R.mipmap.ic_launcher_foreground), null,
            Modifier.size(180.dp),
        )
        Text("카드를 연결하고 사진을 가져오세요", color = p.text, fontSize = 20.sp, fontWeight = FontWeight.SemiBold)
        Spacer(Modifier.height(10.dp))
        Text(
            "SD 카드를 꽂거나 USB 카드 리더를 연결하면, 날짜별로 정리해 이 기기에 복사합니다.\n" +
                "셀렉 도우미가 흔들리거나 눈 감은 컷을 먼저 찾아 드려요.",
            color = p.text2, fontSize = 14.sp, lineHeight = 21.sp,
            modifier = Modifier.widthIn(max = 460.dp),
            textAlign = androidx.compose.ui.text.style.TextAlign.Center,
        )
        Spacer(Modifier.height(22.dp))
        Button(onClick = act::import, colors = ButtonDefaults.buttonColors(containerColor = p.accent)) {
            Icon(Icons.Outlined.Download, null, Modifier.size(18.dp))
            Spacer(Modifier.width(8.dp))
            Text("사진 가져오기")
        }
    }
}

@Composable
fun Message(title: String, text: String) {
    val p = LocalPalette.current
    Column(
        Modifier.fillMaxSize().padding(32.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
    ) {
        Text(title, color = p.text, fontSize = 18.sp, fontWeight = FontWeight.SemiBold)
        Spacer(Modifier.height(8.dp))
        Text(
            text, color = p.text2, fontSize = 14.sp, maxLines = 6, overflow = TextOverflow.Ellipsis,
            textAlign = androidx.compose.ui.text.style.TextAlign.Center,
            modifier = Modifier.widthIn(max = 420.dp),
        )
    }
}
