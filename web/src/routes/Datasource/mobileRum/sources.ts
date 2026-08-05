import type { Source } from '../sources';

const ENDPOINT = '{{ENDPOINT}}';
const TOKEN = '{{TOKEN}}';
const APPLICATION_ID = '{{APPLICATION_ID}}';

export const MOBILE_RUM_SOURCES: Source[] = [
  {
    id: 'rum',
    name: 'Web RUM',
    category: 'recommended',
    glyph: 'WEB',
    description: '浏览器真实用户监控：页面性能、前端错误、资源与用户会话。',
    signals: ['logs'],
    rumPlatform: 'browser',
    steps: [
      {
        title: '1. 安装浏览器 SDK',
        code: { lang: 'bash', content: 'npm i @molesignal/browser-rum' },
      },
      {
        title: '2. 初始化应用',
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
      {
        title: '3. 上传生产 Source Map',
        description:
          '在「RUM 设置 → Source Maps 与 Symbols」上传 .map，应用 ID、service 和 release 必须与事件完全一致。',
        note: '不要把 RUM Client Token 用于上传；调试产物上传需要已登录且具备数据流配置权限。',
      },
      {
        title: '4. 验证错误事件',
        description: '触发一条测试错误，然后在 RUM 错误列表确认应用、版本和源码位置。',
      },
    ],
  },
  {
    id: 'rum-flutter',
    name: 'Flutter RUM',
    category: 'recommended',
    glyph: 'FLT',
    glyphColor: '#46a5e5',
    description: '使用 MoleSignal Flutter SDK 采集页面、交互、错误、资源、慢帧与会话回放。',
    signals: ['logs'],
    rumPlatform: 'flutter',
    docsUrl: 'https://docs.molesignal.io/en-US/rum/flutter-sdk',
    steps: [
      {
        title: '1. 安装 Flutter SDK',
        code: { lang: 'bash', content: 'flutter pub add molesignal_flutter' },
      },
      {
        title: '2. 初始化并安装导航观察器',
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
        title: '3. 生成 Flutter AOT symbols',
        description: '每个发布版本和架构都要保留对应 .symbols 文件。',
        code: {
          lang: 'bash',
          content: `flutter build appbundle --release --obfuscate \\
  --split-debug-info=build/molesignal-symbols

flutter build ipa --release --obfuscate \\
  --split-debug-info=build/molesignal-symbols`,
        },
      },
      {
        title: '4. 上传并验证 symbols',
        description:
          '在「RUM 设置 → Source Maps 与 Symbols」选择 Flutter Symbols，填写相同应用 ID、service、release、平台和架构，逐个上传 .symbols。',
        note: '当前 SDK 的生产堆栈仍需按下方 Flutter SDK 修复清单补齐相对地址和构建标识，才能稳定命中 AOT symbols。',
      },
      {
        title: '5. 验证会话与错误',
        description: '运行 release 包触发测试异常，确认 RUM 会话、错误和回放均归属当前应用。',
      },
    ],
  },
  {
    id: 'rum-android',
    name: 'Android 原生 RUM',
    category: 'recommended',
    glyph: 'AND',
    glyphColor: '#5fc26a',
    description: 'Android 原生应用通过 RUM HTTP 协议上报会话与错误，并支持 R8/NDK 堆栈还原。',
    signals: ['logs'],
    rumPlatform: 'android',
    docsUrl: 'https://developer.android.com/build/shrink-code',
    steps: [
      {
        title: '1. 接入 RUM HTTP 协议',
        description:
          'Android 原生 SDK 发布前，可先用现有网络层接入。以下演示 errors；sessions、actions、replay 分别发送到 /api/v1/rum/sessions、/api/v1/rum/actions、/api/v1/rum/replay，且必须使用同一 application ID。',
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
      {
        title: '2. 保留 R8 mapping.txt',
        description:
          'Release 构建后保存 app/build/outputs/mapping/release/mapping.txt；每次发布都会变化。',
      },
      {
        title: '3. 保留 NDK 未剥离符号',
        description:
          '包含 C/C++ 时，保留各 ABI 的未剥离 .so。上传前可 gzip，但不要只上传已 strip 的 APK 内 .so。',
        code: {
          lang: 'kotlin',
          content: `android {
  buildTypes.release.ndk.debugSymbolLevel = "FULL"
}`,
        },
      },
      {
        title: '4. 上报 NDK 帧地址',
        description:
          'Native 崩溃帧必须包含模块、指令地址、镜像加载地址和 Build ID；服务端会换算为模块相对地址。',
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
      {
        title: '5. 上传 Android 调试产物',
        description:
          '在「RUM 设置 → Source Maps 与 Symbols」分别上传 Android mapping.txt 与 Android Native Symbols；填写应用 ID、版本、ABI 和 Build ID。',
      },
      {
        title: '6. 验证混淆堆栈',
        description: '使用 release 包触发测试异常，确认类名、方法名和源码行号已还原。',
      },
    ],
  },
  {
    id: 'rum-ios',
    name: 'iOS 原生 RUM',
    category: 'recommended',
    glyph: 'iOS',
    glyphColor: '#8f96a3',
    description: 'iOS 原生应用通过 RUM HTTP 协议上报错误地址，并使用 dSYM 恢复函数与源码位置。',
    signals: ['logs'],
    rumPlatform: 'ios',
    docsUrl:
      'https://developer.apple.com/documentation/xcode/adding-identifiable-symbol-names-to-a-crash-report/',
    steps: [
      {
        title: '1. 接入 RUM HTTP 协议',
        description:
          'iOS 原生 SDK 发布前，可先通过 URLSession 接入。以下演示 errors；sessions、actions、replay 分别发送到 /api/v1/rum/sessions、/api/v1/rum/actions、/api/v1/rum/replay，且必须使用同一 application ID。',
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
        title: '2. 归档并保留 dSYM',
        description: 'Xcode Archive 后保留 .xcarchive；应用二进制和 dSYM UUID 必须一致。',
        code: {
          lang: 'bash',
          content: `dwarfdump --uuid MyApp.app.dSYM
gzip -k MyApp.app.dSYM/Contents/Resources/DWARF/MyApp`,
        },
      },
      {
        title: '3. 上传 dSYM DWARF 文件',
        description:
          '在「RUM 设置 → Source Maps 与 Symbols」选择 Apple dSYM，上传 dSYM 包内 Contents/Resources/DWARF/<AppName> 文件或其 .gz，并填写 UUID。',
      },
      {
        title: '4. 验证 release 堆栈',
        description: '使用 TestFlight 或 release 设备包触发测试错误，确认函数、文件和行号已恢复。',
      },
    ],
  },
];
