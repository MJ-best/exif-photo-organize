package io.github.mj_best.photoorganize

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.viewModels
import io.github.mj_best.photoorganize.ui.AppViewModel
import io.github.mj_best.photoorganize.ui.PhotoOrganizeApp
import io.github.mj_best.photoorganize.ui.PhotoTheme

class MainActivity : ComponentActivity() {
    private val vm: AppViewModel by viewModels()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        setContent { PhotoTheme { PhotoOrganizeApp(vm) } }
    }
}
