package com.arzeck.aftertone.engine

import android.util.Log

/**
 * JNI bindings for AfterTone Native Engine.
 * Loads `libaftertone_engine.so`.
 */
object AfterToneNative {
    private const val TAG = "AfterToneNative"
    private var isLoaded = false

    init {
        try {
            System.loadLibrary("aftertone_native")
            isLoaded = true
            Log.i(TAG, "Loaded libaftertone_native successfully (v${getVersion()})")
        } catch (e: UnsatisfiedLinkError) {
            Log.w(TAG, "libaftertone_native not found, native acceleration unavailable: ${e.message}")
            isLoaded = false
        }
    }

    val available: Boolean get() = isLoaded

    external fun startServer(port: Int): Int
    external fun isServerRunning(): Boolean
    external fun stopServer()
    external fun decryptSaavn(url: String): String
    external fun parseTtml(ttml: String): String
    external fun getVersion(): String
}
