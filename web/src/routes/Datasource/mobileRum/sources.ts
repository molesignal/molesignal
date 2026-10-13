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
          content: `import { initRum } from '@molesignal/browser-rum';

initRum({
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
      version: '1.4.0+42',
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
  --split-debug-info=build/molesignal-symbols

flutter build ipa --release --obfuscate \\
  --split-debug-info=build/molesignal-symbols`,
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
          content: `val event = JSONArray().put(JSONObject().apply {
  put("application", "${APPLICATION_ID}")
  put("service", "android-app")
  put("version", BuildConfig.VERSION_NAME + "+" + BuildConfig.VERSION_CODE)
  put("platform", "android")
  put("architecture", Build.SUPPORTED_ABIS.firstOrNull())
  put("timestamp", System.currentTimeMillis() * 1000)
  put("message", throwable.message ?: throwable.javaClass.name)
  put("error", JSONObject().put("stack", JSONArray(
    throwable.stackTrace.map { frame -> JSONObject().apply {
      put("class_name", frame.className)
      put("function", frame.methodName)
      put("line", frame.lineNumber)
    }}
  )))
})

val request = Request.Builder()
  .url("${ENDPOINT}/api/v1/rum/errors")
  .header("Authorization", "Bearer ${TOKEN}")
  .post(event.toString().toRequestBody("application/json".toMediaType()))
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
  if let uuid = imageUUID(at: imageBase) { frame["uuid"] = uuid }
  return frame
}
let event: [[String: Any]] = [[
  "application": "${APPLICATION_ID}",
  "service": "ios-app",
  "version": version + "+" + build,
  "platform": "ios",
  "architecture": "arm64",
  "timestamp": Int(Date().timeIntervalSince1970 * 1_000_000),
  "message": error.localizedDescription,
  "error": ["stack": frames]
]]

var request = URLRequest(url: URL(string: "${ENDPOINT}/api/v1/rum/errors")!)
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
