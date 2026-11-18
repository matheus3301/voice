/* The render thread only copies samples. Pitch detection runs in a Worker. */
class PitchCaptureProcessor extends AudioWorkletProcessor {
  constructor(options) {
    super();
    this.windowSize = options.processorOptions?.windowSize || 4096;
    this.hopSize = 1024;
    this.ring = new Float32Array(this.windowSize);
    this.writeIndex = 0;
    this.total = 0;
    this.sinceMessage = 0;
  }
  process(inputs) {
    const input = inputs[0]?.[0];
    if (!input) return true;
    for (let i = 0; i < input.length; i++) {
      this.ring[this.writeIndex] = input[i];
      this.writeIndex = (this.writeIndex + 1) % this.windowSize;
    }
    this.total += input.length;
    this.sinceMessage += input.length;
    if (this.total >= this.windowSize && this.sinceMessage >= this.hopSize) {
      this.sinceMessage = 0;
      const samples = new Float32Array(this.windowSize);
      samples.set(this.ring.subarray(this.writeIndex));
      samples.set(
        this.ring.subarray(0, this.writeIndex),
        this.windowSize - this.writeIndex,
      );
      const centerTime =
        (currentFrame + input.length - this.windowSize / 2) / sampleRate;
      this.port.postMessage({ samples, sampleRate, centerTime }, [
        samples.buffer,
      ]);
    }
    return true;
  }
}
registerProcessor("pitch-capture", PitchCaptureProcessor);
