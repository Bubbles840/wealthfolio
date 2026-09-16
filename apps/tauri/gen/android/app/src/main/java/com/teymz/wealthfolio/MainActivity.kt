package com.teymz.wealthfolio

import android.os.Bundle
import android.content.Context
import androidx.activity.enableEdgeToEdge
import androidx.annotation.Keep

class MainActivity : TauriActivity() {
  private external fun initializeSecretStoreContext(context: Context)
  private external fun lockProfile()

  private var resumed = false
  private var safeProfileFrame = true

  override fun onPause() {
    resumed = false
    safeProfileFrame = false
    window.addFlags(android.view.WindowManager.LayoutParams.FLAG_SECURE)
    lockProfile()
    super.onPause()
  }

  override fun onResume() {
    super.onResume()
    resumed = true
    if (safeProfileFrame) {
      window.clearFlags(android.view.WindowManager.LayoutParams.FLAG_SECURE)
    }
  }

  // Called on the UI thread after the renderer acknowledges the current privacy cover.
  @Keep
  fun onProfileCoverReady() {
    safeProfileFrame = true
    if (resumed) {
      window.clearFlags(android.view.WindowManager.LayoutParams.FLAG_SECURE)
    }
  }

  override fun onCreate(savedInstanceState: Bundle?) {
    System.loadLibrary("wealthfolio_app_lib")
    initializeSecretStoreContext(applicationContext)
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
  }
}
