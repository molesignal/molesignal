/*
 * What each module draws once it has data, as a small picture for its empty
 * page. They are previews, not charts: no numbers, no labels, only the shape a
 * reader will recognise later. Colours are the data palette's own tokens, so
 * they follow the theme. All of them are decorative (the panel that holds them
 * is hidden from assistive technology).
 */
const ART_CLASS = 'ms-wipe-in block h-auto w-full max-w-[400px]';

/** A trace: spans cascading across a request, one of them an error. */
export function TraceArt() {
  const spans: Array<[x: number, y: number, width: number, color: string, opacity: number]> = [
    [44, 22, 292, 'var(--indigo)', 0.9],
    [60, 50, 120, 'var(--blue)', 0.8],
    [76, 78, 64, 'var(--blue)', 0.55],
    [148, 106, 96, 'var(--green)', 0.8],
    [188, 134, 132, 'var(--red)', 0.75],
    [204, 162, 60, 'var(--green)', 0.55],
  ];
  return (
    <svg viewBox="0 0 360 200" className={ART_CLASS} role="presentation">
      {[44, 117, 190, 263, 336].map((x) => (
        <line key={x} x1={x} y1="10" x2={x} y2="188" stroke="var(--bd-1)" strokeDasharray="2 5" />
      ))}
      {spans.map(([x, y, width, color, opacity], index) => (
        <g key={index}>
          <rect x="8" y={y + 4} width={22 - (index % 3) * 4} height="6" rx="3" fill="var(--bd-1)" />
          <rect x={x} y={y} width={width} height="14" rx="4" fill={color} opacity={opacity} />
        </g>
      ))}
    </svg>
  );
}

/** A page as its users met it: a browser frame, the three vitals, the trend of the experience. */
export function VitalsArt() {
  const gauges: Array<[cx: number, color: string, sweep: number]> = [
    [86, 'var(--green)', 0.82],
    [180, 'var(--yellow)', 0.58],
    [274, 'var(--green)', 0.9],
  ];
  const radius = 26;
  const circumference = 2 * Math.PI * radius;
  // Each gauge is three quarters of a circle, opening downwards.
  const arc = circumference * 0.75;
  return (
    <svg viewBox="0 0 360 200" className={ART_CLASS} role="presentation">
      <rect x="6" y="6" width="348" height="188" rx="12" fill="none" stroke="var(--bd-1)" />
      <line x1="6" y1="34" x2="354" y2="34" stroke="var(--bd-1)" />
      {[24, 38, 52].map((cx) => (
        <circle key={cx} cx={cx} cy="20" r="3.5" fill="var(--bd-1)" />
      ))}
      <rect x="76" y="14" width="208" height="12" rx="6" fill="var(--bd-0)" />
      {gauges.map(([cx, color, sweep]) => (
        <g key={cx} transform={`rotate(135 ${cx} 88)`}>
          <circle
            cx={cx}
            cy="88"
            r={radius}
            fill="none"
            stroke="var(--bd-1)"
            strokeWidth="6"
            strokeLinecap="round"
            strokeDasharray={`${arc} ${circumference}`}
          />
          <circle
            cx={cx}
            cy="88"
            r={radius}
            fill="none"
            stroke={color}
            strokeWidth="6"
            strokeLinecap="round"
            strokeDasharray={`${arc * sweep} ${circumference}`}
          />
        </g>
      ))}
      <path
        d="M28 168 C 60 150, 84 176, 116 160 S 172 138, 204 152 S 262 172, 292 146 S 324 150, 334 142"
        fill="none"
        stroke="var(--indigo)"
        strokeWidth="2"
        strokeLinecap="round"
        opacity="0.8"
      />
    </svg>
  );
}

/** A flame graph: where the time went, widest at the root. */
export function FlameArt() {
  const rows: Array<Array<[x: number, width: number, color: string, opacity: number]>> = [
    [[8, 344, 'var(--blue)', 0.5]],
    [
      [8, 212, 'var(--blue)', 0.38],
      [224, 128, 'var(--yellow)', 0.6],
    ],
    [
      [8, 92, 'var(--yellow)', 0.55],
      [104, 116, 'var(--orange)', 0.6],
      [224, 70, 'var(--yellow)', 0.45],
    ],
    [
      [8, 48, 'var(--yellow)', 0.4],
      [104, 84, 'var(--orange)', 0.8],
      [224, 38, 'var(--orange)', 0.5],
    ],
    [
      [104, 52, 'var(--red)', 0.8],
      [160, 22, 'var(--orange)', 0.45],
    ],
    [[104, 30, 'var(--red)', 0.6]],
  ];
  return (
    <svg viewBox="0 0 360 200" className={ART_CLASS} role="presentation">
      {rows.map((row, depth) =>
        row.map(([x, width, color, opacity]) => (
          <rect
            key={`${depth}:${x}`}
            x={x}
            y={12 + depth * 30}
            width={width}
            height="24"
            rx="4"
            fill={color}
            opacity={opacity}
          />
        )),
      )}
    </svg>
  );
}

/** A status page: the verdict at the top, then each component with the days behind it. */
export function StatusArt() {
  const rows: Array<[y: number, incident: number | null, color: string]> = [
    [78, null, ''],
    [116, 21, 'var(--orange)'],
    [154, 9, 'var(--blue)'],
  ];
  const days = 28;
  return (
    <svg viewBox="0 0 360 200" className={ART_CLASS} role="presentation">
      <rect x="6" y="6" width="348" height="188" rx="12" fill="none" stroke="var(--bd-1)" />
      <rect x="20" y="20" width="320" height="38" rx="8" fill="var(--green)" opacity="0.14" />
      <circle cx="42" cy="39" r="9" fill="var(--green)" opacity="0.85" />
      <path
        d="M37.5 39.5 l3 3 l6 -6.5"
        fill="none"
        stroke="var(--bg-1)"
        strokeWidth="2"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
      <rect x="60" y="33" width="132" height="12" rx="6" fill="var(--green)" opacity="0.55" />
      {rows.map(([y, incident, color]) => (
        <g key={y}>
          <rect x="20" y={y - 14} width={incident === null ? 84 : 62} height="7" rx="3.5" fill="var(--bd-1)" />
          {Array.from({ length: days }, (_, day) => (
            <rect
              key={day}
              x={20 + day * 11.5}
              y={y}
              width="8"
              height="18"
              rx="2"
              fill={day === incident ? color : 'var(--green)'}
              opacity={day === incident ? 0.9 : 0.5}
            />
          ))}
        </g>
      ))}
    </svg>
  );
}

/** An alert rule: a series that crosses its threshold, and the stretch it stays above it. */
export function ThresholdArt() {
  const line = 'M12 150 C 46 138, 70 156, 104 132 S 150 118, 176 96 S 214 44, 242 52 S 278 96, 300 118 S 334 138, 348 132';
  return (
    <svg viewBox="0 0 360 200" className={ART_CLASS} role="presentation">
      {[40, 90, 140].map((y) => (
        <line key={y} x1="12" y1={y} x2="348" y2={y} stroke="var(--bd-1)" strokeDasharray="2 5" />
      ))}
      {/* The stretch the series spends above the threshold is the incident. */}
      <rect x="170" y="14" width="118" height="172" rx="6" fill="var(--red)" opacity="0.08" />
      <line x1="12" y1="104" x2="348" y2="104" stroke="var(--red)" strokeWidth="1.5" strokeDasharray="5 5" opacity="0.7" />
      <path d={line} fill="none" stroke="var(--indigo)" strokeWidth="2.5" strokeLinecap="round" opacity="0.85" />
      <circle cx="170" cy="104" r="5" fill="var(--bg-1)" stroke="var(--red)" strokeWidth="2.5" />
      <circle cx="288" cy="104" r="5" fill="var(--bg-1)" stroke="var(--green)" strokeWidth="2.5" />
      <rect x="170" y="176" width="118" height="6" rx="3" fill="var(--red)" opacity="0.7" />
    </svg>
  );
}

/** A check: one target reached from three places, one of which has stopped getting through. */
export function ProbeArt() {
  const origins: Array<[y: number, color: string, failedFrom: number | null]> = [
    [46, 'var(--green)', null],
    [100, 'var(--green)', null],
    [154, 'var(--red)', 5],
  ];
  return (
    <svg viewBox="0 0 360 200" className={ART_CLASS} role="presentation">
      {origins.map(([y, color, failedFrom]) => (
        <g key={y}>
          {/* What each place has seen, run by run. */}
          {Array.from({ length: 8 }, (_, run) => {
            const failed = failedFrom !== null && run >= failedFrom;
            return (
              <rect
                key={run}
                x={14 + run * 10}
                y={y - 9}
                width="6"
                height="18"
                rx="2"
                fill={failed ? 'var(--red)' : 'var(--green)'}
                opacity={failed ? 0.9 : 0.5}
              />
            );
          })}
          <path
            d={`M104 ${y} C 170 ${y}, 200 100, 262 100`}
            fill="none"
            stroke={color}
            strokeWidth="2"
            strokeLinecap="round"
            opacity="0.75"
            {...(failedFrom !== null ? { strokeDasharray: '3 6' } : {})}
          />
          <circle cx="104" cy={y} r="5" fill="var(--bg-1)" stroke={color} strokeWidth="2.5" />
        </g>
      ))}
      <rect x="262" y="70" width="84" height="60" rx="10" fill="var(--indigo)" opacity="0.12" />
      <rect x="262" y="70" width="84" height="60" rx="10" fill="none" stroke="var(--indigo)" strokeWidth="1.5" opacity="0.6" />
      <rect x="276" y="86" width="40" height="7" rx="3.5" fill="var(--indigo)" opacity="0.7" />
      <rect x="276" y="100" width="56" height="7" rx="3.5" fill="var(--bd-1)" />
      <rect x="276" y="114" width="28" height="7" rx="3.5" fill="var(--bd-1)" />
    </svg>
  );
}
