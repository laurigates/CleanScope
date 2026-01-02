package com.cleanscope.app

import android.os.Bundle
import android.view.WindowManager
import androidx.activity.enableEdgeToEdge
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat

class MainActivity : TauriActivity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)

    // Enable immersive mode - hide status bar and navigation bar
    enableImmersiveMode()

    // Keep screen on while app is active (useful for endoscope viewing)
    window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
  }

  private fun enableImmersiveMode() {
    // Allow content to extend under system bars
    WindowCompat.setDecorFitsSystemWindows(window, false)

    // Get the insets controller
    val windowInsetsController = WindowCompat.getInsetsController(window, window.decorView)

    // Hide both status bar and navigation bar
    windowInsetsController.hide(WindowInsetsCompat.Type.systemBars())

    // Show bars temporarily when user swipes from edge
    windowInsetsController.systemBarsBehavior =
      WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
  }

  override fun onWindowFocusChanged(hasFocus: Boolean) {
    super.onWindowFocusChanged(hasFocus)
    // Re-enable immersive mode when window regains focus
    if (hasFocus) {
      enableImmersiveMode()
    }
  }
}
