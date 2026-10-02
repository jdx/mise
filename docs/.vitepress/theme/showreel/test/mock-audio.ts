// from jdx/hk@37937824 docs/.vitepress/theme/showreel/test/mock-audio.ts
// A stand-in for an OfflineAudioContext that builds no audio and records
// every call the score makes on it, so tests can run the score in Node and
// check what it schedules, and when.

/** One recorded call: a node or param method, a property set (`type=`), or a connection. */
export interface Call {
  target: string;
  method: string;
  args: unknown[];
}

const PARAM_METHODS = [
  "setValueAtTime",
  "linearRampToValueAtTime",
  "exponentialRampToValueAtTime",
  "setTargetAtTime",
  "setValueCurveAtTime",
  "cancelScheduledValues",
] as const;

/**
 * Typed arrays are recorded by length, sum and lowest value (how deep a
 * gain curve dips), which keeps the log small.
 */
function summarize(v: unknown): unknown {
  if (ArrayBuffer.isView(v) && "length" in v) {
    const a = v as unknown as ArrayLike<number>;
    let sum = 0;
    let min = Infinity;
    for (let i = 0; i < a.length; i++) {
      sum += a[i];
      min = Math.min(min, a[i]);
    }
    return `${v.constructor.name}(${a.length}, sum ${sum}, min ${min})`;
  }
  if (v && typeof v === "object" && "id" in v) return (v as { id: unknown }).id;
  return v;
}

export class MockContext {
  readonly calls: Call[] = [];
  readonly currentTime = 0;
  readonly destination: object;
  private n = 0;

  constructor(readonly sampleRate = 48000) {
    this.destination = this.node("destination");
  }

  /** Present, so the score schedules as it does for an offline render. */
  startRendering(): void {}

  /** Sources started, each with the context time it starts at. */
  starts(): { target: string; t: number }[] {
    return this.calls
      .filter((c) => c.method === "start")
      .map((c) => ({ target: c.target, t: c.args[0] as number }));
  }

  /** This context as the Web Audio type the score takes. */
  get context(): BaseAudioContext {
    return this as unknown as BaseAudioContext;
  }

  createGain = () => this.node("gain");
  createBiquadFilter = () => this.node("biquad");
  createStereoPanner = () => this.node("panner");
  createOscillator = () => this.node("osc");
  createBufferSource = () => this.node("source");
  createWaveShaper = () => this.node("shaper");
  createConvolver = () => this.node("convolver");
  createDynamicsCompressor = () => this.node("compressor");
  createPeriodicWave = () => ({ id: "wave" });
  createBuffer = (channels: number, length: number, sampleRate: number) => {
    const data = Array.from(
      { length: channels },
      () => new Float32Array(length),
    );
    return {
      id: "buffer",
      numberOfChannels: channels,
      length,
      sampleRate,
      getChannelData: (c: number) => data[c],
    };
  };

  private record(target: string, method: string, args: unknown[]): void {
    this.calls.push({ target, method, args: args.map(summarize) });
  }

  private param(id: string): object {
    const p: Record<string, unknown> = { id };
    let value = 0;
    Object.defineProperty(p, "value", {
      get: () => value,
      set: (v: number) => {
        value = v;
        this.record(id, "value=", [v]);
      },
    });
    for (const m of PARAM_METHODS) {
      p[m] = (...args: unknown[]) => {
        this.record(id, m, args);
        return p;
      };
    }
    return p;
  }

  /** A node: its methods are recorded, and any other property is an AudioParam. */
  private node(kind: string): object {
    const id = `${kind}#${this.n++}`;
    const props = new Map<string, unknown>();
    const params = new Map<string, object>();
    const call =
      (method: string, ret?: (args: unknown[]) => unknown) =>
      (...args: unknown[]) => {
        this.record(id, method, args);
        return ret?.(args);
      };
    const methods: Record<string, (...args: unknown[]) => unknown> = {
      connect: call("connect", (args) => args[0]),
      disconnect: call("disconnect"),
      start: call("start"),
      stop: call("stop"),
      setPeriodicWave: call("setPeriodicWave"),
    };
    return new Proxy(
      { id },
      {
        get: (_target, prop) => {
          if (typeof prop !== "string") return undefined;
          if (prop === "id") return id;
          if (prop in methods) return methods[prop];
          if (props.has(prop)) return props.get(prop);
          let p = params.get(prop);
          if (!p) {
            p = this.param(`${id}.${prop}`);
            params.set(prop, p);
          }
          return p;
        },
        set: (_target, prop, value) => {
          props.set(String(prop), value);
          this.record(id, `${String(prop)}=`, [value]);
          return true;
        },
      },
    );
  }
}
