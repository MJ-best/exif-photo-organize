package io.github.mj_best.photoorganize.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Slider
import androidx.compose.material3.SliderDefaults
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.mj_best.photoorganize.core.LearnStatus

@Composable
fun DeleteDialog(ask: DeleteAsk, onConfirm: () -> Unit, onDismiss: () -> Unit) {
    val p = LocalPalette.current
    AlertDialog(
        onDismissRequest = onDismiss,
        containerColor = p.panel,
        title = { Text("사진 ${Fmt.count(ask.plan.photos)}장을 휴지통으로 옮길까요?", fontWeight = FontWeight.SemiBold) },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Text(
                    "파일 ${Fmt.count(ask.plan.files)}개(${Fmt.bytes(ask.plan.bytes)})가 라이브러리에서 빠져 휴지통으로 갑니다 — " +
                        "RAW · JPEG · XMP까지 함께. 휴지통은 설정에서 비울 수 있어요.",
                    color = p.text2,
                )
                Text("거절 · 픽 · 별점은 셀렉 도우미의 학습에 그대로 남습니다.", color = p.text3, fontSize = 13.sp)
            }
        },
        confirmButton = {
            Button(onClick = onConfirm, colors = ButtonDefaults.buttonColors(containerColor = p.danger)) {
                Text("휴지통으로 옮기기")
            }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text("취소") } },
    )
}

@Composable
fun SettingsDialog(
    threshold: Int,
    learn: LearnStatus?,
    trashBytes: Long,
    libraryPath: String,
    version: String,
    onThreshold: (Int) -> Unit,
    onEmptyTrash: () -> Unit,
    onDismiss: () -> Unit,
) {
    val p = LocalPalette.current
    var t by remember(threshold) { mutableFloatStateOf(threshold.toFloat()) }
    var confirmEmpty by remember { mutableStateOf(false) }
    AlertDialog(
        onDismissRequest = onDismiss,
        containerColor = p.panel,
        title = { Text("설정", fontWeight = FontWeight.SemiBold) },
        text = {
            Column(Modifier.verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                Text("셀렉 도우미", color = p.text, fontWeight = FontWeight.SemiBold)
                Text(learn?.summary ?: "기본 기준", color = p.text2, fontSize = 14.sp)
                if (learn != null) {
                    Text(
                        "본 사진 ${Fmt.count(learn.analysed)} / ${Fmt.count(learn.total)}장 · " +
                            "거절 · ★1은 B컷, 픽 · ★3 이상은 A컷으로 배웁니다.",
                        color = p.text3, fontSize = 13.sp,
                    )
                }
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text("B컷 후보 기준", color = p.text2, modifier = Modifier.weight(1f))
                    Text("${t.toInt()}%", color = p.text, fontWeight = FontWeight.SemiBold)
                }
                Slider(
                    value = t,
                    onValueChange = { t = it },
                    onValueChangeFinished = { onThreshold(t.toInt()) },
                    valueRange = 30f..90f,
                    steps = 11,
                    colors = SliderDefaults.colors(thumbColor = p.accent, activeTrackColor = p.accent),
                )
                Text("낮추면 더 많이 걸러내고, 높이면 확실한 것만 보여요.", color = p.text3, fontSize = 13.sp)
                HorizontalDivider(color = p.separator)
                Text("휴지통", color = p.text, fontWeight = FontWeight.SemiBold)
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(if (trashBytes > 0) Fmt.bytes(trashBytes) else "비어 있어요", color = p.text2, modifier = Modifier.weight(1f))
                    if (trashBytes > 0) {
                        TextButton(onClick = { confirmEmpty = true }) { Text("비우기", color = p.danger) }
                    }
                }
                if (confirmEmpty) {
                    Text("휴지통의 사진을 완전히 지울까요? 되돌릴 수 없어요.", color = p.danger, fontSize = 13.sp)
                    Row {
                        TextButton(onClick = { confirmEmpty = false }) { Text("취소") }
                        TextButton(onClick = {
                            confirmEmpty = false
                            onEmptyTrash()
                        }) { Text("완전히 지우기", color = p.danger) }
                    }
                }
                HorizontalDivider(color = p.separator)
                Text("라이브러리", color = p.text, fontWeight = FontWeight.SemiBold)
                Text(libraryPath, color = p.text2, fontSize = 12.sp)
                Text(
                    "앱을 지우면 이 폴더의 사진도 함께 지워집니다. 고른 사진은 컴퓨터로 옮겨 두세요 — " +
                        "별점 · 거절은 XMP 사이드카에 있어 LOUPE 데스크톱 앱과 라이트룸에서 그대로 보입니다.",
                    color = p.text3, fontSize = 13.sp,
                )
                HorizontalDivider(color = p.separator)
                Text("LOUPE $version — 사진가의 서랍, 사진을 고르는 시간", color = p.text2, fontSize = 13.sp)
                Text(
                    "사진은 기기 밖으로 나가지 않습니다 (인터넷 권한 없음). 얼굴 인식 YuNet(MIT), 장면 인식 MobileNetV2(Apache-2.0) 모델을 씁니다.",
                    color = p.text3, fontSize = 12.sp,
                )
            }
        },
        confirmButton = { TextButton(onClick = onDismiss) { Text("닫기") } },
    )
}
