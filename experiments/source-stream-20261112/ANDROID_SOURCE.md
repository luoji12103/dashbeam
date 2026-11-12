# Android URI source experiment

## Scope and integration boundary

`ExperimentalUriSource.kt` is an unused, internal helper for opening one
`content://` URI read-only. It uses `openFileDescriptor`, checks the opened
descriptor with `fstat` for a regular file and a nonnegative size, and seeks
to offset zero before lending the descriptor and its size to a synchronous
callback. Unsupported source types, including provider pipes, fail explicitly;
the helper never copies a file into the app cache. It conservatively rejects
tree URIs, including document URIs represented through a tree URI. Folder and
multiple-file sharing are outside this experiment.

The existing picker and share-intent paths do not call this helper. They still
copy to `cache/file_cache` and pass the completed paths through the frontend to
the existing blob import. The helper is not a completed Android transfer or a
claim that the application now avoids staging.

The callback borrows the descriptor only for its duration. Any future JNI
bridge must synchronously duplicate it inside the callback and transfer sole
ownership of that duplicate to the native stream task; it must close that
duplicate on every success, failure and cancellation path. Do not return the
borrowed descriptor or `/proc/self/fd/N` as a path, and do not transfer an
integer fd to a different process. A `dup` shares the underlying file position:
the native source must be the only reader/seeker. The intended future native
interface accepts an owned `std::fs::File`, a validated display name, and send
options. The current stream prototype targets a known-size, seekable regular
file and must check for source changes; this helper does not provide snapshot
or mutation protection itself.

The caller must already hold the URI read grant when opening the source.
Permission is not persisted or extended here. Opening and reading can block;
use an appropriate worker and do not assume coroutine cancellation or closing
another reference immediately interrupts a blocked provider read. The
provider can change its contents; fstat size is only a momentary observation.
The existing application behavior remains necessary until an actual picker,
JNI ownership bridge, lifecycle, cancellation, size handling and receiver path
are integrated and tested.

## Compile check, 2026-11-12

On `SER8-PVE-VM-102-Debian13-Dev` (`debian13-dev`), the source was copied to
the task-owned directory
`/home/luoji/code/dashbeam-source-stream-20261112.wXzKWL/android-validation`.
This standalone Kotlin compile exited 0 and produced
`out/com/dashbeam/plugin/native_utils/ExperimentalUriSourceKt.class`:

```sh
CACHE=/home/luoji/.gradle/caches/modules-2/files-2.1
CP=$(find "$CACHE/org.jetbrains.kotlin/kotlin-compiler-embeddable/2.2.10" "$CACHE/org.jetbrains.kotlin/kotlin-stdlib/2.2.10" "$CACHE/org.jetbrains.kotlin/kotlin-script-runtime/2.2.10" "$CACHE/org.jetbrains.kotlin/kotlin-reflect/1.6.10" "$CACHE/org.jetbrains.kotlin/kotlin-daemon-embeddable/2.2.10" "$CACHE/org.jetbrains.kotlinx/kotlinx-coroutines-core-jvm/1.8.0" "$CACHE/org.jetbrains/annotations" -name "*.jar" -type f 2>/dev/null | paste -sd:)
STDLIB=$(find "$CACHE/org.jetbrains.kotlin/kotlin-stdlib/2.2.10" -name "*.jar" -type f | head -1)
/home/luoji/.gradle/jdks/jetbrains_s_r_o_-21-amd64-linux.2/bin/java \
  -Xmx384m -cp "$CP" org.jetbrains.kotlin.cli.jvm.K2JVMCompiler \
  -no-stdlib -no-reflect \
  -classpath "/home/luoji/Android/Sdk/platforms/android-36/android.jar:$STDLIB" \
  -d /home/luoji/code/dashbeam-source-stream-20261112.wXzKWL/android-validation/out \
  /home/luoji/code/dashbeam-source-stream-20261112.wXzKWL/android-validation/ExperimentalUriSource.kt
```

The JDK reported JetBrains Runtime 21.0.11+10-b1163.116. SHA-256 values:

| Input | SHA-256 |
| --- | --- |
| Source compiled, also matching the Mac workspace copy | `839d1e4d907dccc26421a0d40dd77484f80cfc29fb56d8fd235d12e1e1f5486f` |
| Kotlin compiler embeddable 2.2.10 JAR | `79628fd5d64b4692a62f98962baab00e9076a37e2cffd6ace33cd0b7be7d1cdf` |
| Kotlin stdlib 2.2.10 JAR | `9c67cc79efd6b9215b49d2a4308f5f3433537376c7c88e89bdd6729bd096e61a` |
| Android API 36 `android.jar` | `d9eb9da824d9e247a352f570f01e1169e725b2954bca9e283a71786c59b59f9a` |

This is a source/API compile check only. No Android runtime, real provider,
descriptor ownership/cancellation test, APK build, actual device transfer, or
resource measurement was performed.
