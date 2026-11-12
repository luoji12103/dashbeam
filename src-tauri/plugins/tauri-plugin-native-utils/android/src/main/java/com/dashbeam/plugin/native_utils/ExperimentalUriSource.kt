package com.dashbeam.plugin.native_utils

import android.content.ContentResolver
import android.content.Context
import android.net.Uri
import android.provider.DocumentsContract
import android.system.ErrnoException
import android.system.Os
import android.system.OsConstants
import java.io.FileDescriptor
import java.io.FileNotFoundException

/**
 * Experimental bridge for a single content URI; this does not start a transfer.
 *
 * [useDescriptor] receives a borrowed descriptor and its current size. The
 * descriptor is valid only during
 * the call. A native consumer that outlives the callback must synchronously
 * duplicate it and own and close the duplicate on every success and error path.
 * Neither the descriptor nor a path such as /proc/self/fd/N may be retained
 * after the callback. The consumer must run in this process.
 *
 * Only seekable regular files are accepted; pipes and other provider streams
 * fail explicitly. The descriptor is positioned at zero before the callback.
 * The size is a snapshot, not a guarantee that the content remains unchanged.
 * Opening and reading can block; call this on a suitable worker and do not
 * assume coroutine cancellation interrupts a blocked provider read.
 *
 * The caller is responsible for having a valid URI read grant when this runs.
 * This function does not persist or extend permissions and does not make the
 * underlying content immutable. Provider, permission and callback failures
 * propagate to the caller.
 */
internal fun <T> withExperimentalUriReadDescriptor(
    context: Context,
    uri: Uri,
    useDescriptor: (FileDescriptor, Long) -> T,
): T {
    require(uri.scheme == ContentResolver.SCHEME_CONTENT && !uri.authority.isNullOrBlank()) {
        "Expected a content URI"
    }
    require(!DocumentsContract.isTreeUri(uri)) {
        "A directory tree is not a single file"
    }

    val descriptor = context.contentResolver.openFileDescriptor(uri, "r")
        ?: throw FileNotFoundException("Provider returned no file descriptor")
    return descriptor.use {
        val fileDescriptor = it.fileDescriptor
        val stat = Os.fstat(fileDescriptor)
        if (!OsConstants.S_ISREG(stat.st_mode) || stat.st_size < 0) {
            throw UnsupportedOperationException("Provider source is not a regular file with a known size")
        }
        try {
            if (Os.lseek(fileDescriptor, 0L, OsConstants.SEEK_SET) != 0L) {
                throw UnsupportedOperationException("Provider source could not be positioned at zero")
            }
        } catch (error: ErrnoException) {
            throw UnsupportedOperationException("Provider source is not seekable", error)
        }
        useDescriptor(fileDescriptor, stat.st_size)
    }
}
