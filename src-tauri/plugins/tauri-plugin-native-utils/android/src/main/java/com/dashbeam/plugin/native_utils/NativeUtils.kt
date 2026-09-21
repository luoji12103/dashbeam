package com.dashbeam.plugin.native_utils

import android.app.Activity
import android.app.DownloadManager
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.ClipData
import android.content.ClipboardManager
import android.content.ContentResolver
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.Environment
import android.provider.DocumentsContract
import android.provider.OpenableColumns
import android.webkit.WebView
import androidx.activity.result.ActivityResult
import androidx.annotation.Keep
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import app.tauri.annotation.ActivityCallback
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Channel
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.cancelAndJoin
import kotlinx.coroutines.launch
import java.io.ByteArrayOutputStream
import java.io.File
import java.io.IOException
import java.io.InputStream
import java.io.OutputStream
import java.net.HttpURLConnection
import java.net.URL
import java.nio.ByteBuffer
import java.nio.CharBuffer
import java.nio.charset.CharacterCodingException
import java.nio.charset.CodingErrorAction
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.ConcurrentLinkedQueue
import java.util.concurrent.atomic.AtomicBoolean

@InvokeArg
class SelectorArgs {
    lateinit var channel: Channel
}

@InvokeArg
class CancelJobArgs(
    var channelId: Long = 0
)

@InvokeArg
class ExportToTreeArgs {
    var treeUri: String = ""
    var sourceDir: String = ""
}

@InvokeArg
class OpenDownloadFolderArgs {
    var treeUri: String = ""
}

@InvokeArg
class ExportToMediaStoreArgs {
    var sourceDir: String = ""
}

@InvokeArg
class WriteTextToUriArgs {
    var uri: String = ""
    var contents: String = ""
}

@InvokeArg
class WriteClipboardTextArgs {
    var text: String = ""
}

@InvokeArg
class OpenDownloadTargetArgs {
    var uri: String = ""

    /**
     * Destination relative to external storage, e.g. `Download/DashBeam`, for
     * when there's no single file to show. Empty falls back to Downloads.
     */
    var relativePath: String = ""
}

@Keep
data class DownloadFolderSelectionResponse(
    val uri: String,
    val path: String,
)

@Keep
data class SelectedTextDocumentResponse(
    val text: String,
    val fileName: String,
)

@Keep
data class SharedTextResponse(
    val text: String? = null,
    val error: String? = null,
)

@TauriPlugin
class NativeUtils(private val activity: Activity) : Plugin(activity) {
    private val scope = CoroutineScope(Dispatchers.IO + SupervisorJob())
    private val jobs = ConcurrentHashMap<Long, Pair<Job, String>>()
    private val pendingShareBatches = ConcurrentLinkedQueue<List<Uri>>()
    private val pendingSharedText = ConcurrentLinkedQueue<SharedTextResponse>()
    private val pendingReceivedTextTap = AtomicBoolean(false)
    @Volatile
    private var isActivityResumed = false

    companion object {
        private const val RW_PERMISSION_FLAGS =
            Intent.FLAG_GRANT_WRITE_URI_PERMISSION or Intent.FLAG_GRANT_READ_URI_PERMISSION
        private const val SHARE_RECEIVED_EVENT = "shareReceived"
        private const val TEXT_SHARE_RECEIVED_EVENT = "textShareReceived"
        private const val RECEIVED_TEXT_TAP_EVENT = "receivedTextNotificationTapped"
        private const val RECEIVED_TEXT_TAP_EXTRA =
            "com.dashbeam.plugin.native_utils.RECEIVED_TEXT_NOTIFICATION_TAP"

        private const val TEXT_MAX_BYTES = 1024 * 1024
        private const val RECEIVED_TEXT_CHANNEL_ID = "dashbeam_received_text"
        private const val RECEIVED_TEXT_NOTIFICATION_ID = 4712

        /** Sentinel the Rust side matches to fall back to app-private staging. */
        const val MEDIA_STORE_UNSUPPORTED = "MEDIA_STORE_UNSUPPORTED"

        /** Documents provider backing the primary shared-storage volume. */
        private const val EXTERNAL_STORAGE_AUTHORITY =
            "com.android.externalstorage.documents"

        /**
         * The manifest the desktop updater already reads, kept here rather than
         * taken as an argument so this cannot become a general-purpose fetch
         * primitive for the webview. Mirrors
         * `tauri.conf.json` -> `plugins.updater.endpoints`.
         */
        private const val LATEST_MANIFEST_URL =
            "https://github.com/tonyantony300/dashbeam/releases/latest/download/latest.json"

        private const val MANIFEST_TIMEOUT_MS = 15_000

        /** Plenty for a manifest; stops a wrong URL from streaming into memory. */
        private const val MANIFEST_MAX_BYTES = 512 * 1024
    }

    @Command
    fun select_download_folder(invoke: Invoke) = startActivityForResult(
        invoke,
        Intent(Intent.ACTION_OPEN_DOCUMENT_TREE),
        this::handleDownloadFolderSelection.name
    )

    @Command
    fun select_send_document(invoke: Invoke) = startActivityForResult(
        invoke,
        Intent(Intent.ACTION_OPEN_DOCUMENT).apply {
            type = "*/*"
        },
        this::handleSendSelection.name
    )

    @Command
    fun select_send_folder(invoke: Invoke) = startActivityForResult(
        invoke,
        Intent(Intent.ACTION_OPEN_DOCUMENT_TREE),
        this::handleSendSelection.name
    )

    @Command
    fun select_send_markdown(invoke: Invoke) = startActivityForResult(
        invoke,
        Intent(Intent.ACTION_OPEN_DOCUMENT).apply {
            addCategory(Intent.CATEGORY_OPENABLE)
            // Some document providers report .md as application/octet-stream.
            // Visibility is broad; the callback still requires a .md name and
            // validates bounded, strict UTF-8 before returning any content.
            type = "*/*"
            putExtra(
                Intent.EXTRA_MIME_TYPES,
                arrayOf("text/markdown", "text/plain", "application/octet-stream")
            )
        },
        this::handleMarkdownSelection.name
    )

    /** Android 10+ restricts background clipboard access. Enforce the same
     * policy on every supported API so a background WebView cannot write. */
    @Command
    fun write_clipboard_text(invoke: Invoke) {
        val args = invoke.parseArgs(WriteClipboardTextArgs::class.java)
        val checkedText = try {
            validateAndMeasureText(args.text)
            args.text
        } catch (e: Exception) {
            return invoke.reject(e.message ?: "Clipboard text is invalid")
        }

        activity.runOnUiThread {
            if (!isActivityResumed || activity.isFinishing || activity.isDestroyed) {
                invoke.reject("Clipboard writes are only allowed while DashBeam is in the foreground")
                return@runOnUiThread
            }

            try {
                val clipboard = activity.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
                clipboard.setPrimaryClip(ClipData.newPlainText("", checkedText))
                invoke.resolve()
            } catch (e: Exception) {
                invoke.reject(e.message ?: "Failed to write to the clipboard")
            }
        }
    }

    @Command
    fun consume_shared_text(invoke: Invoke) {
        captureShareIntent(activity.intent)
        val shared = pendingSharedText.poll()
        if (shared == null) invoke.resolve(null) else invoke.resolveObject(shared)
    }

    @Command
    fun consume_received_text_notification_tap(invoke: Invoke) {
        captureReceivedTextNotificationTap(activity.intent)
        invoke.resolveObject(pendingReceivedTextTap.getAndSet(false))
    }

    /** Best effort by design: notification denial must not fail the receive. */
    @Command
    fun notify_received_text(invoke: Invoke) {
        // The foreground UI presents the received text directly; avoid a
        // duplicate alert and reserve notifications for background delivery.
        if (isActivityResumed) {
            invoke.resolveObject(false)
            return
        }

        val notifications = NotificationManagerCompat.from(activity)
        if (!notifications.areNotificationsEnabled()) {
            invoke.resolveObject(false)
            return
        }

        try {
            createReceivedTextNotificationChannel()
            notifications.notify(RECEIVED_TEXT_NOTIFICATION_ID, buildReceivedTextNotification())
            invoke.resolveObject(true)
        } catch (_: SecurityException) {
            // Permission can be revoked between the check and notify().
            invoke.resolveObject(false)
        } catch (e: Exception) {
            invoke.reject(e.message ?: "Failed to show received-text notification")
        }
    }

    @Command
    fun consume_share_intent(invoke: Invoke) {
        val args = invoke.parseArgs(SelectorArgs::class.java)
        val uris = takePendingOrIntentShare()
            ?: return invoke.resolveObject(false)

        startUriCopy(uris, args.channel)
        invoke.resolveObject(true)
    }

    @Command
    fun cancel_job(invoke: Invoke) {
        val args = invoke.parseArgs(CancelJobArgs::class.java)
        val channelId = args.channelId
        val (job, tempFolder) = jobs.remove(channelId)
            ?: return invoke.reject("Trying to cancel a non existing job")
        scope.launch {
            try {
                job.cancelAndJoin()
                File(tempFolder).deleteRecursively()
                invoke.resolve()
            } catch (e: Exception) {
                invoke.reject(e.message)
            }
        }
    }

    @Command
    fun export_to_tree(invoke: Invoke) {
        val args = invoke.parseArgs(ExportToTreeArgs::class.java)
        scope.launch {
            try {
                val treeUri = Uri.parse(args.treeUri)
                val sourceDir = File(args.sourceDir)
                val result = exportDirectoryToTree(activity, treeUri, sourceDir)
                invoke.resolveObject(result)
            } catch (e: SecurityException) {
                invoke.reject(e.message ?: "SAF permission denied")
            } catch (e: Exception) {
                invoke.reject(e.message ?: "Failed to export to selected folder")
            }
        }
    }

    /**
     * Presence lifetime is decided in Rust, which knows the paired-device count
     * and the discoverability setting; these two just relay the decision.
     */
    @Command
    fun start_presence_service(invoke: Invoke) {
        PresenceService.start(activity.applicationContext)
        invoke.resolve()
    }

    @Command
    fun stop_presence_service(invoke: Invoke) {
        PresenceService.stop(activity.applicationContext)
        invoke.resolve()
    }

    @Command
    fun get_window_insets(invoke: Invoke) {
        activity.runOnUiThread {
            try {
                invoke.resolveObject(activity.readWindowInsets())
            } catch (e: Exception) {
                invoke.reject(e.message ?: "Failed to read window insets")
            }
        }
    }

    @Command
    fun open_download_folder(invoke: Invoke) {
        val args = invoke.parseArgs(OpenDownloadFolderArgs::class.java)
        val treeUriString = args.treeUri.trim()
        if (treeUriString.isEmpty()) {
            return invoke.reject("No download folder URI available")
        }

        try {
            val treeUri = Uri.parse(treeUriString)
            if (!DocumentsContract.isTreeUri(treeUri)) {
                return invoke.reject("Invalid download folder URI")
            }

            val docId = DocumentsContract.getTreeDocumentId(treeUri)
            val documentUri = DocumentsContract.buildDocumentUriUsingTree(treeUri, docId)
            val intent = Intent(Intent.ACTION_VIEW).apply {
                setDataAndType(documentUri, DocumentsContract.Document.MIME_TYPE_DIR)
                addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
                addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            }

            try {
                activity.startActivity(intent)
            } catch (_: android.content.ActivityNotFoundException) {
                activity.startActivity(Intent.createChooser(intent, null))
            }
            invoke.resolve()
        } catch (e: Exception) {
            invoke.reject(e.message ?: "Failed to open download folder")
        }
    }

    @Command
    fun export_to_media_store(invoke: Invoke) {
        val args = invoke.parseArgs(ExportToMediaStoreArgs::class.java)
        scope.launch {
            try {
                val result = exportDirectoryToMediaStore(activity, File(args.sourceDir))
                invoke.resolveObject(result)
            } catch (_: MediaStoreUnsupportedException) {
                invoke.reject(MEDIA_STORE_UNSUPPORTED)
            } catch (e: Exception) {
                invoke.reject(e.message ?: "Failed to export to the Downloads folder")
            }
        }
    }

    /**
     * Show a received file, the folder it landed in, or the system Downloads
     * list. There's no tree URI for the SAF path and `ACTION_VIEW_DOWNLOADS`
     * only ever opens the Downloads root, so a document URI built from
     * `relativePath` is tried first — OEM file managers vary, hence the chain.
     */
    @Command
    fun open_download_target(invoke: Invoke) {
        val args = invoke.parseArgs(OpenDownloadTargetArgs::class.java)
        val uriString = args.uri.trim()
        val relativePath = args.relativePath.trim().trim('/')

        try {
            val candidates = mutableListOf<Intent>()

            if (uriString.isNotEmpty()) {
                val uri = Uri.parse(uriString)
                val mime = activity.contentResolver.getType(uri) ?: "*/*"
                candidates += Intent(Intent.ACTION_VIEW).apply {
                    setDataAndType(uri, mime)
                    addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
                }
            }

            candidates += folderIntents(relativePath)
            candidates += Intent(DownloadManager.ACTION_VIEW_DOWNLOADS)

            if (!startFirstResolvable(candidates)) {
                invoke.reject("No app available to open the downloaded files")
                return
            }
            invoke.resolve()
        } catch (e: Exception) {
            invoke.reject(e.message ?: "Failed to open the downloaded file")
        }
    }

    /**
     * Write UTF-8 text to a SAF document URI. `ACTION_CREATE_DOCUMENT` (what
     * the dialog plugin's `save()` runs) creates the file and returns a
     * `content://` URI, which is not a path — `std::fs::write` on it fails and
     * leaves the freshly created 0-byte document behind.
     */
    @Command
    fun write_text_to_uri(invoke: Invoke) {
        val args = invoke.parseArgs(WriteTextToUriArgs::class.java)
        val uriString = args.uri.trim()
        if (uriString.isEmpty()) {
            return invoke.reject("No destination URI provided")
        }

        scope.launch {
            try {
                val stream = openForOverwrite(Uri.parse(uriString))
                    ?: return@launch invoke.reject("Could not open $uriString for writing")
                stream.use { it.write(args.contents.toByteArray(Charsets.UTF_8)) }
                invoke.resolve()
            } catch (e: SecurityException) {
                invoke.reject(e.message ?: "No permission to write to the selected file")
            } catch (e: Exception) {
                invoke.reject(e.message ?: "Failed to write to the selected file")
            }
        }
    }

    /**
     * Fetch the release manifest over Android's own TLS stack. Rust cannot do
     * this here: reqwest verifies through `rustls-platform-verifier`, which
     * needs a JVM handshake this app never performs. Rust parses the body.
     */
    @Command
    fun fetch_update_manifest(invoke: Invoke) {
        scope.launch {
            var connection: HttpURLConnection? = null
            try {
                connection = (URL(LATEST_MANIFEST_URL).openConnection() as HttpURLConnection).apply {
                    requestMethod = "GET"
                    connectTimeout = MANIFEST_TIMEOUT_MS
                    readTimeout = MANIFEST_TIMEOUT_MS
                    // github.com 302s release assets to another host; both
                    // legs are https, so the built-in follower handles it.
                    instanceFollowRedirects = true
                    setRequestProperty("Accept", "application/json")
                }

                val status = connection.responseCode
                if (status !in 200..299) {
                    return@launch invoke.reject("Update manifest request failed ($status)")
                }

                val body = connection.inputStream.use { readBounded(it, MANIFEST_MAX_BYTES) }

                invoke.resolveObject(body)
            } catch (e: Exception) {
                invoke.reject(e.message ?: "Could not reach the update server")
            } finally {
                connection?.disconnect()
            }
        }
    }

    /**
     * "wt" truncates, which matters when the user picks an existing file to
     * overwrite — plain "w" would leave the tail of anything longer behind.
     * Not every documents provider implements the mode, so "w" is the fallback.
     */
    private fun openForOverwrite(uri: Uri): OutputStream? {
        val resolver = activity.contentResolver
        return try {
            resolver.openOutputStream(uri, "wt")
        } catch (_: IllegalArgumentException) {
            resolver.openOutputStream(uri, "w")
        } catch (_: UnsupportedOperationException) {
            resolver.openOutputStream(uri, "w")
        }
    }

    /** Read UTF-8 up to `limit` bytes, failing rather than truncating silently. */
    private fun readBounded(input: InputStream, limit: Int): String {
        val buffer = ByteArrayOutputStream()
        val chunk = ByteArray(8 * 1024)
        while (true) {
            val read = input.read(chunk)
            if (read == -1) break
            if (buffer.size() + read > limit) {
                throw IOException("Update manifest larger than $limit bytes")
            }
            buffer.write(chunk, 0, read)
        }
        return buffer.toString(Charsets.UTF_8.name())
    }

    private fun readStrictUtf8(input: InputStream, limit: Int): String {
        val buffer = ByteArrayOutputStream()
        val chunk = ByteArray(8 * 1024)
        while (true) {
            val read = input.read(chunk)
            if (read == -1) break
            if (buffer.size() + read > limit) {
                throw IOException("Text is larger than $limit bytes")
            }
            buffer.write(chunk, 0, read)
        }

        return Charsets.UTF_8.newDecoder()
            .onMalformedInput(CodingErrorAction.REPORT)
            .onUnmappableCharacter(CodingErrorAction.REPORT)
            .decode(ByteBuffer.wrap(buffer.toByteArray()))
            .toString()
            .removePrefix("\uFEFF")
    }

    /** Reject malformed UTF-16 (unpaired surrogates) and enforce the same
     * byte limit used by the transfer protocol before retaining shared text. */
    private fun validateAndMeasureText(text: String) {
        if (text.length > TEXT_MAX_BYTES) {
            throw IOException("Text is larger than $TEXT_MAX_BYTES bytes")
        }
        val bytes = Charsets.UTF_8.newEncoder()
            .onMalformedInput(CodingErrorAction.REPORT)
            .onUnmappableCharacter(CodingErrorAction.REPORT)
            .encode(CharBuffer.wrap(text))
            .remaining()
        if (bytes > TEXT_MAX_BYTES) {
            throw IOException("Text is larger than $TEXT_MAX_BYTES bytes")
        }
    }

    private fun queryDisplayName(uri: Uri): String? {
        activity.contentResolver.query(
            uri,
            arrayOf(OpenableColumns.DISPLAY_NAME),
            null,
            null,
            null,
        )?.use { cursor ->
            if (cursor.moveToFirst()) {
                val index = cursor.getColumnIndex(OpenableColumns.DISPLAY_NAME)
                if (index >= 0) return cursor.getString(index)
            }
        }
        return uri.lastPathSegment?.substringAfterLast('/')
    }

    private fun createReceivedTextNotificationChannel() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return
        val manager = activity.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
        manager.createNotificationChannel(
            NotificationChannel(
                RECEIVED_TEXT_CHANNEL_ID,
                activity.getString(R.string.received_text_channel_name),
                NotificationManager.IMPORTANCE_DEFAULT,
            ).apply {
                description = activity.getString(R.string.received_text_channel_description)
                setShowBadge(true)
            }
        )
    }

    private fun buildReceivedTextNotification(): android.app.Notification {
        val launchIntent = activity.packageManager.getLaunchIntentForPackage(activity.packageName)
            ?: Intent(activity, activity::class.java)
        launchIntent.apply {
            addFlags(Intent.FLAG_ACTIVITY_CLEAR_TOP or Intent.FLAG_ACTIVITY_SINGLE_TOP)
            putExtra(RECEIVED_TEXT_TAP_EXTRA, true)
        }
        val tapIntent = PendingIntent.getActivity(
            activity,
            RECEIVED_TEXT_NOTIFICATION_ID,
            launchIntent,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )

        return NotificationCompat.Builder(activity, RECEIVED_TEXT_CHANNEL_ID)
            .setSmallIcon(R.drawable.ic_presence_notification)
            .setContentTitle(activity.getString(R.string.received_text_notification_title))
            // Deliberately no body, BigTextStyle, ticker, or payload extras.
            .setCategory(NotificationCompat.CATEGORY_MESSAGE)
            .setPriority(NotificationCompat.PRIORITY_DEFAULT)
            .setAutoCancel(true)
            .setContentIntent(tapIntent)
            .build()
    }

    /**
     * `ACTION_VIEW` intents pointing at a storage folder, most specific first.
     * Two document-URI shapes, because file managers disagree on which they
     * accept — a bare document URI or one built against a tree.
     */
    private fun folderIntents(relativePath: String): List<Intent> {
        if (relativePath.isEmpty()) return emptyList()

        val documentId = "primary:$relativePath"
        val authority = EXTERNAL_STORAGE_AUTHORITY
        val treeUri = DocumentsContract.buildTreeDocumentUri(authority, documentId)

        return listOf(
            DocumentsContract.buildDocumentUriUsingTree(treeUri, documentId),
            DocumentsContract.buildDocumentUri(authority, documentId),
        ).map { uri ->
            Intent(Intent.ACTION_VIEW).apply {
                setDataAndType(uri, DocumentsContract.Document.MIME_TYPE_DIR)
                addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
            }
        }
    }

    /**
     * Fire the first intent something can handle, reporting whether any did.
     * `resolveActivity` is unreliable under API 30 package visibility, so each
     * is simply attempted in turn.
     */
    private fun startFirstResolvable(intents: List<Intent>): Boolean {
        for (intent in intents) {
            intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            try {
                activity.startActivity(intent)
                return true
            } catch (_: android.content.ActivityNotFoundException) {
                // Try the next, less specific, target.
            }
        }
        return false
    }

    @ActivityCallback
    fun handleDownloadFolderSelection(invoke: Invoke, result: ActivityResult) {
        if (Activity.RESULT_OK != result.resultCode) return invoke.resolve(null)

        val uri = result.data?.data ?: return invoke.resolve(null)

        try {
            activity.contentResolver.takePersistableUriPermission(uri, RW_PERMISSION_FLAGS)

            invoke.resolveObject(
                DownloadFolderSelectionResponse(
                    uri.toString(),
                    uri.extractFolderOsPath(),
                )
            )

            activity.contentResolver.persistedUriPermissions.stream()
                .filter { it.uri != uri }
                .forEach {
                    activity.contentResolver.releasePersistableUriPermission(
                        it.uri,
                        RW_PERMISSION_FLAGS
                    )
                }
        } catch (e: Exception) {
            invoke.reject(e.message)
        }
    }

    @ActivityCallback
    fun handleSendSelection(invoke: Invoke, result: ActivityResult) {
        val args = invoke.parseArgs(SelectorArgs::class.java)
        val channel = args.channel

        if (Activity.RESULT_OK != result.resultCode) return invoke.resolveObject(false)

        val uri = result.data?.data ?: return invoke.resolveObject(false)

        startUriCopy(listOf(uri), channel)
        invoke.resolveObject(true)
    }

    @ActivityCallback
    fun handleMarkdownSelection(invoke: Invoke, result: ActivityResult) {
        if (Activity.RESULT_OK != result.resultCode) return invoke.resolve(null)
        val uri = result.data?.data ?: return invoke.resolve(null)

        scope.launch {
            try {
                val fileName = queryDisplayName(uri)
                    ?: throw IOException("Could not read the selected document name")
                if (!fileName.endsWith(".md", ignoreCase = true)) {
                    throw IOException("Select a Markdown (.md) file")
                }

                val stream = activity.contentResolver.openInputStream(uri)
                    ?: throw IOException("Could not open the selected document")
                val text = stream.use { readStrictUtf8(it, TEXT_MAX_BYTES) }
                invoke.resolveObject(SelectedTextDocumentResponse(text, fileName))
            } catch (e: SecurityException) {
                invoke.reject(e.message ?: "No permission to read the selected document")
            } catch (e: CharacterCodingException) {
                invoke.reject("The selected Markdown file is not valid UTF-8")
            } catch (e: Exception) {
                invoke.reject(e.message ?: "Failed to import the selected Markdown file")
            }
        }
    }

    override fun load(webView: WebView) {
        super.load(webView)
        // Cold start: capture all supported payloads before / as the frontend mounts.
        // Skip wiping file_cache when a file share is pending so cleanup cannot race the copy.
        captureShareIntent(activity.intent)
        captureReceivedTextNotificationTap(activity.intent)
        if (pendingShareBatches.isEmpty()) {
            scope.launch {
                activity.cacheDir.resolve("file_cache").deleteRecursively()
            }
        }
        // Commands also poll the queues, so a listener registration race cannot lose data.
        webView.post { advertisePendingEvents() }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        // Without this, activity.intent stays the old MAIN launcher intent under singleTask.
        activity.intent = intent

        captureShareIntent(intent)
        captureReceivedTextNotificationTap(intent)
        advertisePendingEvents()
    }

    override fun onResume() {
        super.onResume()
        isActivityResumed = true
        // Safety net: if the frontend missed the first event (listener not ready yet),
        // re-advertise any still-unconsumed payload when we come to the foreground.
        captureShareIntent(activity.intent)
        captureReceivedTextNotificationTap(activity.intent)
        advertisePendingEvents()
    }

    override fun onPause() {
        isActivityResumed = false
        super.onPause()
    }

    override fun onDestroy() {
        jobs.forEach { _, (job, tempFolder) ->
            try {
                job.cancel()
                File(tempFolder).deleteRecursively()
            } catch (_: Exception) {
            }
        }

        scope.cancel()
        super.onDestroy()
    }

    private fun startUriCopy(uris: List<Uri>, channel: Channel) {
        val path = listOf(
            activity.cacheDir.absolutePath,
            "file_cache",
            System.currentTimeMillis().toString(),
        ).joinToString(File.separator)

        val tempFolder = File(path)
        val job = scope.launch(start = CoroutineStart.LAZY) {
            try {
                tempFolder.parentFile?.mkdirs()
                    ?: throw IOException("Unable to create parent folders for ${tempFolder.absolutePath}")

                if (uris.size == 1) {
                    copyUri(activity, uris.single(), tempFolder).collect {
                        channel.send(it.toJSObject())
                    }
                } else {
                    val totalBytes = uris.sumOf { resolveContentLength(activity, it) }
                    channel.send(CopyProgress(0, totalBytes, tempFolder.absolutePath).toJSObject())

                    var copiedBytes = 0L
                    val cachedPaths = mutableListOf<String>()
                    uris.forEachIndexed { index, uri ->
                        var fileBytes = 0L
                        copyUri(activity, uri, tempFolder.resolve(index.toString())).collect { progress ->
                            fileBytes = progress.copiedBytes
                            progress.cachedPath?.takeIf { progress.completed }?.let(cachedPaths::add)
                            channel.send(
                                CopyProgress(
                                    copiedBytes + progress.copiedBytes,
                                    totalBytes,
                                    null,
                                ).toJSObject()
                            )
                        }
                        copiedBytes += fileBytes
                    }

                    channel.send(
                        CopyProgress(
                            copiedBytes,
                            if (totalBytes > 0) totalBytes else copiedBytes,
                            null,
                            cachedPaths,
                            completed = true,
                        ).toJSObject()
                    )
                }
            } catch (e: Exception) {
                tempFolder.deleteRecursively()
                channel.send(
                    JSObject().apply {
                        put("error", e.message ?: "Failed to copy shared file")
                        put("progress", -1.0)
                        put("copiedBytes", "0")
                        put("totalBytes", "0")
                    }
                )
            } finally {
                jobs.remove(channel.id)
            }
        }

        jobs[channel.id] = job to tempFolder.absolutePath
        job.start()
    }

    @Synchronized
    private fun takePendingOrIntentShare(): List<Uri>? {
        captureShareIntent(activity.intent)
        return pendingShareBatches.poll()
    }

    @Synchronized
    private fun captureShareIntent(intent: Intent?) {
        if (intent == null ||
            (intent.action != Intent.ACTION_SEND && intent.action != Intent.ACTION_SEND_MULTIPLE)
        ) {
            return
        }

        extractShareUris(intent).takeIf { it.isNotEmpty() }?.let(pendingShareBatches::add)
        extractSharedText(intent)?.let(pendingSharedText::add)
        // Consume only after both independent payload kinds have been captured.
        intent.action = null
    }

    private fun extractSharedText(intent: Intent): SharedTextResponse? {
        val mime = intent.type?.substringBefore(';')?.lowercase()
        if (intent.action != Intent.ACTION_SEND ||
            mime?.startsWith("text/") != true
        ) return null
        // EXTRA_TEXT is the canonical plain CharSequence representation even
        // for text/html. Never read or render EXTRA_HTML_TEXT here.
        val raw = intent.getCharSequenceExtra(Intent.EXTRA_TEXT)?.toString() ?: return null
        return try {
            validateAndMeasureText(raw)
            SharedTextResponse(text = raw)
        } catch (e: Exception) {
            SharedTextResponse(error = e.message ?: "Shared text is invalid")
        }
    }

    private fun advertisePendingEvents() {
        if (pendingShareBatches.isNotEmpty()) trigger(SHARE_RECEIVED_EVENT, JSObject())
        if (pendingSharedText.isNotEmpty()) trigger(TEXT_SHARE_RECEIVED_EVENT, JSObject())
        if (pendingReceivedTextTap.get()) trigger(RECEIVED_TEXT_TAP_EVENT, JSObject())
    }

    private fun captureReceivedTextNotificationTap(intent: Intent?) {
        if (intent?.getBooleanExtra(RECEIVED_TEXT_TAP_EXTRA, false) != true) return
        intent.removeExtra(RECEIVED_TEXT_TAP_EXTRA)
        pendingReceivedTextTap.set(true)
    }

    private fun extractShareUris(intent: Intent): List<Uri> {
        val uris = mutableListOf<Uri>()
        parcelableStreamExtras(intent)?.let(uris::addAll)

        when (val stream = intent.extras?.get(Intent.EXTRA_STREAM)) {
            is Uri -> uris.add(stream)
            is String -> if (stream.isNotBlank()) uris.add(Uri.parse(stream))
            is List<*> -> uris.addAll(stream.filterIsInstance<Uri>())
        }

        val clip = intent.clipData
        if (clip != null) {
            repeat(clip.itemCount) { index ->
                clip.getItemAt(index)?.uri?.let(uris::add)
            }
        }

        intent.data?.let(uris::add)

        return uris.distinct().filter { it.scheme == ContentResolver.SCHEME_CONTENT }
    }

    @Suppress("DEPRECATION")
    private fun parcelableStreamExtras(intent: Intent): List<Uri>? {
        return if (intent.action == Intent.ACTION_SEND_MULTIPLE) {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                intent.getParcelableArrayListExtra(Intent.EXTRA_STREAM, Uri::class.java)
            } else {
                intent.getParcelableArrayListExtra(Intent.EXTRA_STREAM)
            }
        } else {
            val uri = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                intent.getParcelableExtra(Intent.EXTRA_STREAM, Uri::class.java)
            } else {
                intent.getParcelableExtra(Intent.EXTRA_STREAM) as? Uri
            }
            uri?.let(::listOf)
        }
    }
}

fun Uri.extractFolderOsPath(): String {
    require(DocumentsContract.isTreeUri(this))

    val path = this.path
        ?: throw IOException("Unable to get path from selected download folder uri: $this")
    val baseExternalPath = Environment.getExternalStorageDirectory().path
    return try {
        val docId = DocumentsContract.getTreeDocumentId(this)
        val segments = docId.split(":")
        when {
            "primary" == segments[0] && segments.size > 1 -> "${baseExternalPath}/${segments[1]}"
            "primary" == segments[0] -> baseExternalPath
            segments.size > 1 -> "/storage/${segments[0]}/${segments[1]}"
            else -> "/storage/${segments[0]}/"
        }
    } catch (_: Exception) {
        path
    }
}
