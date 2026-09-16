package com.teymz.wealthfolio

import android.os.Bundle
import android.content.Context
import androidx.activity.enableEdgeToEdge

class MainActivity : TauriActivity() {
  private external fun initializeSecretStoreContext(context: Context)
  private external fun lockProfile()

  override fun onPause() {
    window.addFlags(android.view.WindowManager.LayoutParams.FLAG_SECURE)
    lockProfile()
    super.onPause()
  }

  override fun onCreate(savedInstanceState: Bundle?) {
    System.loadLibrary("wealthfolio_app_lib")
    initializeSecretStoreContext(applicationContext)
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
  }
}
