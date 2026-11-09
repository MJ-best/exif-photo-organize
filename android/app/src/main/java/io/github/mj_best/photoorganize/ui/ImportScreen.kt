package io.github.mj_best.photoorganize.ui

import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.CheckCircle
import androidx.compose.material.icons.outlined.Close
import androidx.compose.material.icons.outlined.SdCard
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Switch
import androidx.compose.material3.SwitchDefaults
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.mj_best.photoorganize.R

interface ImportActions {
    fun pick()
    fun includeVideos(on: Boolean)
    fun start()
    fun cancel()
    fun close()
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ImportScreen(state: ImportUi, act: ImportActions) {
    val p = LocalPalette.current
    Scaffold(
        containerColor = p.content,
        topBar = {
            TopAppBar(
                colors = TopAppBarDefaults.topAppBarColors(containerColor = p.toolbar),
                navigationIcon = {
                    if (state !is ImportUi.Copying) IconButton(onClick = act::close) { Icon(Icons.Outlined.Close, "닫기") }
                },
                title = { Text("사진 가져오기", fontWeight = FontWeight.SemiBold) },
            )
        },
    ) { pad ->
        Box(Modifier.padding(pad).fillMaxSize(), contentAlignment = Alignment.TopCenter) {
            Column(
                Modifier.widthIn(max = 560.dp).fillMaxWidth().verticalScroll(rememberScrollState()).padding(24.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
                verticalArrangement = Arrangement.spacedBy(14.dp),
            ) {
                when (state) {
                    ImportUi.Idle -> Pick(act)
                    ImportUi.Listing -> Busy("카드를 살펴보는 중…", "사진마다 촬영 날짜와 카메라를 읽고 있어요.")
                    is ImportUi.Ready -> Ready(state, act)
                    is ImportUi.Copying -> Copying(state, act)
                    is ImportUi.Done -> Done(state, act)
                    is ImportUi.Failed -> {
                        Text(state.message, color = p.danger, textAlign = TextAlign.Center)
                        Button(onClick = act::pick) { Text("다시 고르기") }
                    }
                }
            }
        }
    }
}

@Composable
private fun Pick(act: ImportActions) {
    val p = LocalPalette.current
    Image(painterResource(R.mipmap.ic_launcher_foreground), null, Modifier.size(150.dp))
    Text("카드에서 사진 가져오기", color = p.text, fontSize = 21.sp, fontWeight = FontWeight.SemiBold)
    Text(
        "SD 카드를 꽂거나 USB 카드 리더를 연결한 뒤, 카드의 DCIM 폴더를 고르세요.",
        color = p.text2, fontSize = 15.sp, textAlign = TextAlign.Center, lineHeight = 22.sp,
    )
    Steps(
        listOf(
            "사진은 촬영 날짜별 폴더로 이 기기에 복사되고, RAW와 JPEG은 한 장으로 묶여요.",
            "복사하면서 체크섬으로 확인하고, 이미 가져온 사진은 건너뜁니다.",
            "카드는 읽기만 하고 아무것도 지우지 않아요.",
        ),
    )
    Button(
        onClick = act::pick,
        colors = ButtonDefaults.buttonColors(containerColor = p.accent),
        modifier = Modifier.padding(top = 6.dp),
    ) {
        Icon(Icons.Outlined.SdCard, null, Modifier.size(18.dp))
        Spacer(Modifier.width(8.dp))
        Text("카드 고르기")
    }
}

@Composable
private fun Steps(lines: List<String>) {
    val p = LocalPalette.current
    Column(
        Modifier.fillMaxWidth().clip(RoundedCornerShape(12.dp)).background(p.panel).padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        lines.forEach { line ->
            Row {
                Text("·", color = p.accent, fontWeight = FontWeight.Bold, modifier = Modifier.width(14.dp))
                Text(line, color = p.text2, fontSize = 14.sp, lineHeight = 20.sp)
            }
        }
    }
}

@Composable
private fun Busy(title: String, text: String) {
    val p = LocalPalette.current
    Spacer(Modifier.height(40.dp))
    CircularProgressIndicator(color = p.accent)
    Text(title, color = p.text, fontSize = 18.sp, fontWeight = FontWeight.SemiBold)
    Text(text, color = p.text2, fontSize = 14.sp, textAlign = TextAlign.Center)
}

@Composable
private fun Ready(state: ImportUi.Ready, act: ImportActions) {
    val p = LocalPalette.current
    val i = state.info
    val already = i.photos - i.newPhotos
    Column(
        Modifier.fillMaxWidth().clip(RoundedCornerShape(14.dp)).background(p.panel).padding(20.dp),
        verticalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        Text(i.label, color = p.text2, fontSize = 13.sp)
        Text(
            if (i.newPhotos > 0) "새 사진 ${Fmt.count(i.newPhotos)}장" else "새 사진이 없어요",
            color = p.text, fontSize = 26.sp, fontWeight = FontWeight.SemiBold,
        )
        if (i.newPhotos > 0) {
            Text("파일 ${Fmt.count(i.newFiles)}개 · ${Fmt.bytes(i.newBytes)}", color = p.text2, fontSize = 15.sp)
        }
        val range = Fmt.dateRange(i.firstMs, i.lastMs)
        if (range.isNotEmpty()) Text(range, color = p.text2, fontSize = 14.sp)
        if (i.rawPairs > 0) Text("RAW+JPEG ${Fmt.count(i.rawPairs)}쌍은 한 장으로 묶어요", color = p.text3, fontSize = 13.sp)
        if (already > 0) Text("이미 가져온 ${Fmt.count(already)}장은 건너뜁니다", color = p.text3, fontSize = 13.sp)
        i.cameras.take(4).forEach { Text(it, color = p.text3, fontSize = 13.sp) }
    }
    Row(
        Modifier.fillMaxWidth().clip(RoundedCornerShape(12.dp)).background(p.panel).padding(horizontal = 16.dp, vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text("동영상도 가져오기", color = p.text, modifier = Modifier.weight(1f))
        Switch(
            checked = state.includeVideos,
            onCheckedChange = act::includeVideos,
            colors = SwitchDefaults.colors(checkedTrackColor = p.accent),
        )
    }
    val tooBig = i.newBytes > state.freeBytes
    if (tooBig) {
        Text(
            "이 기기에 남은 공간(${Fmt.bytes(state.freeBytes)})이 모자라요. 공간을 비운 뒤 다시 시도하세요.",
            color = p.danger, fontSize = 14.sp, textAlign = TextAlign.Center,
        )
    }
    Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
        OutlinedButton(onClick = act::pick) { Text("다른 폴더") }
        Button(
            onClick = act::start,
            enabled = i.newPhotos > 0 && !tooBig,
            colors = ButtonDefaults.buttonColors(containerColor = p.accent),
        ) { Text(if (i.newPhotos > 0) "${Fmt.count(i.newPhotos)}장 가져오기" else "가져오기") }
    }
}

@Composable
private fun Copying(state: ImportUi.Copying, act: ImportActions) {
    val p = LocalPalette.current
    val s = state.status
    // A long copy must not stop because the screen went dark.
    val view = LocalView.current
    DisposableEffect(view) {
        view.keepScreenOn = true
        onDispose { view.keepScreenOn = false }
    }
    Spacer(Modifier.height(30.dp))
    Text("${state.label}에서 가져오는 중", color = p.text, fontSize = 19.sp, fontWeight = FontWeight.SemiBold, textAlign = TextAlign.Center)
    LinearProgressIndicator(
        progress = { if (s.bytesTotal > 0) (s.bytesDone.toFloat() / s.bytesTotal).coerceIn(0f, 1f) else 0f },
        modifier = Modifier.fillMaxWidth().height(8.dp).clip(RoundedCornerShape(4.dp)),
        color = p.accent,
        trackColor = p.separator,
        drawStopIndicator = {},
    )
    Text(
        "파일 ${Fmt.count(s.filesDone)} / ${Fmt.count(s.filesTotal)} · ${Fmt.bytes(s.bytesDone)} / ${Fmt.bytes(s.bytesTotal)}",
        color = p.text2, fontSize = 14.sp,
    )
    Text(s.current, color = p.text3, fontSize = 13.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
    Text("가져오는 동안 앱을 열어 두세요. 화면은 켜진 채로 있어요.", color = p.text3, fontSize = 13.sp, textAlign = TextAlign.Center)
    OutlinedButton(onClick = act::cancel) { Text("중단") }
}

@Composable
private fun Done(state: ImportUi.Done, act: ImportActions) {
    val p = LocalPalette.current
    val r = state.result
    Spacer(Modifier.height(30.dp))
    Icon(Icons.Outlined.CheckCircle, null, tint = p.success, modifier = Modifier.size(56.dp))
    Text(
        when {
            r.cancelled -> "중단했어요 · ${Fmt.count(r.added)}장을 가져왔어요"
            r.added > 0 -> "${Fmt.count(r.added)}장을 가져왔어요"
            else -> "새로 가져온 사진이 없어요"
        },
        color = p.text, fontSize = 21.sp, fontWeight = FontWeight.SemiBold, textAlign = TextAlign.Center,
    )
    val notes = buildList {
        if (r.bytes > 0) add("${Fmt.bytes(r.bytes)} 복사 · 체크섬 확인 완료")
        if (r.updated > 0) add("이미 있던 ${Fmt.count(r.updated)}장에 짝(RAW/JPEG)을 붙였어요")
        if (r.duplicates > 0) add("이미 가져온 ${Fmt.count(r.duplicates)}장은 건너뛰었어요")
        if (r.added > 0) add("셀렉 도우미가 새 사진을 보기 시작합니다")
    }
    notes.forEach { Text(it, color = p.text2, fontSize = 14.sp) }
    if (r.failures.isNotEmpty()) {
        Text("${Fmt.count(r.failures.size)}개는 가져오지 못했어요", color = p.danger, fontSize = 14.sp)
        r.failures.take(5).forEach { Text(it, color = p.text3, fontSize = 12.sp, maxLines = 2) }
    }
    Button(onClick = act::close, colors = ButtonDefaults.buttonColors(containerColor = p.accent)) { Text("사진 보기") }
}
