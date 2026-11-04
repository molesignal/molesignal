import type { Source } from '../sources';

const ENDPOINT = '{{ENDPOINT}}';
const TOKEN = '{{TOKEN}}';
const APPLICATION_ID = '{{APPLICATION_ID}}';

export const MOBILE_RUM_SOURCES: Source[] = [
  {
    id: 'rum',
    category: 'recommended',
    signals: ['logs'],
    rumPlatform: 'browser',
    steps: [
      { code: { lang: 'bash', content: 'npm i @molesignal/browser-rum' } },
      {
        code: {
          lang: 'ts',
          content: `import { init } from '@molesignal/browser-rum';

init({
  applicationId: '${APPLICATION_ID}',
  clientToken: '${TOKEN}',
  site: '${ENDPOINT}',
  service: 'web-frontend',
  env: 'production',
  version: '1.4.0',
  sessionSampleRate: 100,
  trackUserInteractions: true,
});`,
        },
      },
      {},
      { verification: true },
    ],
  },
  {
    id: 'rum-flutter',
    category: 'recommended',
    signals: ['logs'],
    rumPlatform: 'flutter',
    docsUrl: 'https://docs.molesignal.com/en-US/rum/flutter-sdk',
    steps: [
      { code: { lang: 'bash', content: 'flutter pub add molesignal_flutter' } },
      {
        code: {
          lang: 'dart',
          content: `Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  final rum = await initRum(
    const RumConfiguration(
      applicationId: '${APPLICATION_ID}',
      clientToken: '${TOKEN}',
      site: '${ENDPOINT}',
      service: 'checkout-app',
      env: 'production',
      version: String.fromEnvironment('MOLESIGNAL_VERSION'),
      architecture: String.fromEnvironment('MOLESIGNAL_ARCHITECTURE'),
      debugId: String.fromEnvironment('MOLESIGNAL_DEBUG_ID'),
      sessionSampleRate: 100,
      sessionReplaySampleRate: 20,
      trackUserInteractions: true,
    ),
  );

  runApp(RumApp(
    client: rum,
    child: MaterialApp(
      navigatorObservers: <NavigatorObserver>[RumNavigationObserver(rum)],
      home: const CheckoutApp(),
    ),
  ));
}`,
        },
      },
      {
        code: {
          lang: 'bash',
          content: `flutter build appbundle --release --obfuscate \\
  --split-debug-info=build/molesignal-symbols \\
  --dart-define=MOLESIGNAL_VERSION=1.4.0+42 \\
  --dart-define=MOLESIGNAL_ARCHITECTURE=arm64 \\
  --dart-define=MOLESIGNAL_DEBUG_ID="$BUILD_ID"

flutter build ipa --release --obfuscate \\
  --split-debug-info=build/molesignal-symbols \\
  --dart-define=MOLESIGNAL_VERSION=1.4.0+42 \\
  --dart-define=MOLESIGNAL_ARCHITECTURE=arm64 \\
  --dart-define=MOLESIGNAL_DEBUG_ID="$BUILD_ID"`,
        },
      },
      {},
      { verification: true },
    ],
  },
  {
    id: 'rum-android',
    category: 'recommended',
    signals: ['logs'],
    rumPlatform: 'android',
    docsUrl: 'https://developer.android.com/build/shrink-code',
    steps: [
      {
        code: {
          lang: 'kotlin',
          content: `// sessionId, sessionStart, viewId and screenName are yours: a UUID and the start time
// (milliseconds) kept for the session, and a UUID and the name of the screen that is showing.
val event = JSONObject().apply {
  put("type", "error")
  put("date", System.currentTimeMillis())
  put("source", "android")
  put("service", "android-app")
  put("env", "production")
  put("version", BuildConfig.VERSION_NAME)
  put("build_version", BuildConfig.VERSION_CODE.toString())
  put("application", JSONObject().put("id", "${APPLICATION_ID}"))
  put("session", JSONObject().put("id", sessionId).put("type", "user").put("start", sessionStart))
  put("view", JSONObject().put("id", viewId).put("url", screenName).put("name", screenName))
  put("os", JSONObject().put("name", "android").put("version", Build.VERSION.RELEASE))
  put("device", JSONObject().put("architecture", Build.SUPPORTED_ABIS.firstOrNull()))
  put("error", JSONObject().apply {
    put("id", UUID.randomUUID().toString())
    put("message", throwable.message ?: throwable.javaClass.name)
    put("type", throwable.javaClass.name)
    put("source", "source")
    put("handling", "unhandled")
    put("is_crash", true)
    put("source_type", "android")
    put("stack", Log.getStackTraceString(throwable))
    put("frames", JSONArray(throwable.stackTrace.map { frame ->
      JSONObject().apply {
        put("class", frame.className)
        put("function", frame.methodName)
        put("line", frame.lineNumber)
      }
    }))
  })
}

val request = Request.Builder()
  .url("${ENDPOINT}/api/v1/rum/events")
  .header("Authorization", "Bearer ${TOKEN}")
  .post(JSONArray().put(event).toString().toRequestBody("application/json".toMediaType()))
  .build()`,
        },
      },
      {},
      {
        code: {
          lang: 'kotlin',
          content: `android {
  buildTypes.release.ndk.debugSymbolLevel = "FULL"
}`,
        },
      },
      {
        code: {
          lang: 'json',
          content: `{
  "artifact_kind": "android_native_symbols",
  "module": "libcheckout.so",
  "instruction_addr": "0x7a12c4f010",
  "image_addr": "0x7a12c00000",
  "build_id": "aabbccddeeff0011"
}`,
        },
      },
      {},
      { verification: true },
    ],
  },
  {
    id: 'rum-ios',
    category: 'recommended',
    signals: ['logs'],
    rumPlatform: 'ios',
    docsUrl:
      'https://developer.apple.com/documentation/xcode/adding-identifiable-symbol-names-to-a-crash-report/',
    steps: [
      {
        code: {
          lang: 'swift',
          content: `import Darwin
import Foundation
import MachO

func imageUUID(at imageBase: UnsafeRawPointer) -> String? {
  let header = imageBase.assumingMemoryBound(to: mach_header_64.self).pointee
  guard header.magic == MH_MAGIC_64 else { return nil }
  var cursor = imageBase.advanced(by: MemoryLayout<mach_header_64>.size)
  for _ in 0..<header.ncmds {
    let command = cursor.assumingMemoryBound(to: load_command.self).pointee
    guard command.cmdsize >= MemoryLayout<load_command>.size else { return nil }
    if command.cmd == LC_UUID {
      let value = cursor.assumingMemoryBound(to: uuid_command.self).pointee.uuid
      return UUID(uuid: value).uuidString
    }
    cursor = cursor.advanced(by: Int(command.cmdsize))
  }
  return nil
}

let executable = Bundle.main.object(forInfoDictionaryKey: "CFBundleExecutable") as? String ?? "App"
let version = Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "unknown"
let build = Bundle.main.object(forInfoDictionaryKey: "CFBundleVersion") as? String ?? "0"
let frames = Thread.callStackReturnAddresses.compactMap { address -> [String: Any]? in
  let absolute = address.uint64Value
  guard let pointer = UnsafeRawPointer(bitPattern: UInt(absolute)) else { return nil }
  var info = Dl_info()
  guard dladdr(pointer, &info) != 0, let imageBase = info.dli_fbase else { return nil }
  let module = info.dli_fname.map {
    URL(fileURLWithPath: String(cString: $0)).lastPathComponent
  } ?? executable
  var frame: [String: Any] = [
    "artifact_kind": "apple_dsym",
    "module": module,
    "instruction_addr": String(format: "0x%llx", absolute),
    "image_addr": String(
      format: "0x%llx",
      UInt64(UInt(bitPattern: Int(bitPattern: imageBase)))
    )
  ]
  if let uuid = imageUUID(at: imageBase) { frame["debug_id"] = uuid }
  return frame
}
// sessionId, sessionStart and viewId are yours: a UUID and the start time (milliseconds) kept
// for the session, and a UUID for the screen that is showing.
let event: [[String: Any]] = [[
  "type": "error",
  "date": Int(Date().timeIntervalSince1970 * 1000),
  "source": "ios",
  "service": "ios-app",
  "env": "production",
  "version": version,
  "build_version": build,
  "application": ["id": "${APPLICATION_ID}"],
  "session": ["id": sessionId, "type": "user", "start": sessionStart],
  "view": ["id": viewId, "url": "CheckoutViewController", "name": "Checkout"],
  "os": ["name": "ios"],
  "device": ["architecture": "arm64"],
  "error": [
    "id": UUID().uuidString,
    "message": error.localizedDescription,
    "source": "source",
    "handling": "handled",
    "source_type": "ios",
    "frames": frames
  ]
]]

var request = URLRequest(url: URL(string: "${ENDPOINT}/api/v1/rum/events")!)
request.httpMethod = "POST"
request.setValue("Bearer ${TOKEN}", forHTTPHeaderField: "Authorization")
request.setValue("application/json", forHTTPHeaderField: "Content-Type")
request.httpBody = try JSONSerialization.data(withJSONObject: event)
URLSession.shared.dataTask(with: request).resume()`,
        },
      },
      {
        code: {
          lang: 'bash',
          content: `dwarfdump --uuid MyApp.app.dSYM
gzip -k MyApp.app.dSYM/Contents/Resources/DWARF/MyApp`,
        },
      },
      {},
      { verification: true },
    ],
  },
];
