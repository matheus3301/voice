/** Artificial mixtures of real Vocadito vocals; NOT a choir benchmark. */
import { createHash } from 'node:crypto';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { readFileSync, writeFileSync } from 'node:fs';
import { resolve, join } from 'node:path';
import * as tf from '@tensorflow/tfjs';
import { BasicPitch, outputToNotesPoly, noteFramesToTime } from '@spotify/basic-pitch';
import { resampleMono } from '../src/audio/pitch.ts';

const args = process.argv.slice(2);
const option = key => { const index = args.indexOf(key); return index >= 0 ? args[index + 1] : undefined; };
const dataset = option('--dataset');
if (!dataset) throw new Error('Provide --dataset with extracted Vocadito.');
const root = resolve(dataset);
const modelDir = resolve(option('--model') ?? 'public/models/basic-pitch');
const duration = Number(option('--seconds') ?? 8);
const pairCount = Number(option('--pairs') ?? 3);
if (!(duration > 0 && duration <= 8) || ![1, 2, 3].includes(pairCount)) throw new Error('Use --seconds in (0,8] and --pairs 1,2,3.');
const pairs = [[1, 21], [2, 22], [3, 23]].slice(0, pairCount);
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const modelBytes = readFileSync(join(modelDir, 'model.json'));
const modelJson = JSON.parse(modelBytes);
const weightSpecs = modelJson.weightsManifest.flatMap(group => group.weights);
const weightPaths = modelJson.weightsManifest.flatMap(group => group.paths);
const weightBuffers = weightPaths.map(path => readFileSync(join(modelDir, path)));
const concatenatedWeights = Buffer.concat(weightBuffers);
const weights = concatenatedWeights.buffer.slice(concatenatedWeights.byteOffset, concatenatedWeights.byteOffset + concatenatedWeights.byteLength);
await tf.setBackend('cpu');
await tf.ready();
const graphModel = await tf.loadGraphModel(tf.io.fromMemory({ modelTopology: modelJson.modelTopology, weightSpecs, weightData: weights, format: modelJson.format, generatedBy: modelJson.generatedBy, convertedBy: modelJson.convertedBy }));
const basicPitch = new BasicPitch(Promise.resolve(graphModel));
const sampleRate = 22050;
const onsetThreshold = 0.5;
const frameThreshold = 0.3;
const minimumNoteFrames = 5;
// Refuse a misleading benchmark if production's post-processing parameters drift.
const workerBytes = readFileSync(new URL('../src/audio/polyphonic.worker.ts', import.meta.url));
const workerThresholds = workerBytes.toString('utf8').match(/outputToNotesPoly\(frames,\s*onsets,\s*([\d.]+),\s*([\d.]+),\s*(\d+)\)/);
assert.ok(workerThresholds, 'Cannot identify production note thresholds.');
assert.deepEqual(workerThresholds.slice(1).map(Number), [onsetThreshold, frameThreshold, minimumNoteFrames]);
const resamplerBytes = readFileSync(new URL('../src/audio/pitch.ts', import.meta.url));
const basicPitchVersion = JSON.parse(readFileSync(new URL('../node_modules/@spotify/basic-pitch/package.json', import.meta.url), 'utf8')).version;
assert.equal(basicPitchVersion, '1.0.1');
assert.equal(tf.version.tfjs, '3.21.0');
const hop = 0.01;
const empty = () => ({ frames: 0, referencePitches: 0, predictedPitches: 0, truePositive: 0, falsePositive: 0, falseNegative: 0 });
const metrics = c => {
  const precision = c.predictedPitches ? c.truePositive / c.predictedPitches : 0;
  const recall = c.referencePitches ? c.truePositive / c.referencePitches : 0;
  return { precision: precision * 100, recall: recall * 100, f1: precision + recall ? 2 * precision * recall / (precision + recall) * 100 : 0 };
};
function maximumMatches(reference, predicted) {
  const assigned = new Map();
  function match(referenceIndex, visited) {
    for (let predictionIndex = 0; predictionIndex < predicted.length; predictionIndex++) {
      if (Math.abs(1200 * Math.log2(predicted[predictionIndex] / reference[referenceIndex])) > 50 || visited.has(predictionIndex)) continue;
      visited.add(predictionIndex);
      if (!assigned.has(predictionIndex) || match(assigned.get(predictionIndex), visited)) { assigned.set(predictionIndex, referenceIndex); return true; }
    }
    return false;
  }
  return reference.reduce((count, _, index) => count + Number(match(index, new Set())), 0);
}
function addFrame(counts, reference, predicted) {
  const matches = maximumMatches(reference, predicted);
  counts.frames++;
  counts.referencePitches += reference.length;
  counts.predictedPitches += predicted.length;
  counts.truePositive += matches;
  counts.falsePositive += predicted.length - matches;
  counts.falseNegative += reference.length - matches;
}
function readClip(id) {
  const audio = join(root, 'Audio', `vocadito_${id}.wav`);
  const annotation = join(root, 'Annotations', 'F0', `vocadito_${id}_f0.csv`);
  const metadata = JSON.parse(execFileSync('ffprobe', ['-v', 'error', '-select_streams', 'a:0', '-show_entries', 'stream=sample_rate,channels', '-of', 'json', audio], { encoding: 'utf8' })).streams[0];
  const sourceRate = Number(metadata.sample_rate);
  const channelCount = Number(metadata.channels);
  // Decode only: preserve original sample rate/channels, then use the app resampler.
  const decoded = execFileSync('ffmpeg', ['-v', 'error', '-i', audio, '-t', String(duration), '-map', '0:a:0', '-f', 'f32le', '-acodec', 'pcm_f32le', 'pipe:1'], { maxBuffer: 64 * 1024 * 1024 });
  const sampleCount = decoded.length / 4 / channelCount;
  const channels = Array.from({ length: channelCount }, () => new Float32Array(sampleCount));
  for (let sample = 0; sample < sampleCount; sample++) for (let channel = 0; channel < channelCount; channel++) channels[channel][sample] = decoded.readFloatLE((sample * channelCount + channel) * 4);
  const resampled = resampleMono(channels, sourceRate, sampleRate);
  const samples = new Float32Array(Math.round(duration * sampleRate));
  samples.set(resampled.subarray(0, samples.length));
  const f0 = readFileSync(annotation, 'utf8').trim().split(/\r?\n/).map(line => line.split(',').map(Number));
  return { samples, f0, source: { id: `vocadito_${id}`, sourceRate, channelCount, audioSha256: hash(readFileSync(audio)), annotationSha256: hash(readFileSync(annotation)) } };
}
function f0At(rows, time) {
  let low = 0; let high = rows.length;
  while (low < high) { const mid = (low + high) >>> 1; if (rows[mid][0] < time) low = mid + 1; else high = mid; }
  const before = rows[Math.max(0, low - 1)];
  const after = rows[Math.min(rows.length - 1, low)];
  return Math.abs(after[0] - time) < Math.abs(before[0] - time) ? after[1] : before[1];
}
// Sanity checks for the evaluator: no octave equivalence or double-counted predictions.
assert.equal(maximumMatches([440], [880]), 0);
assert.equal(maximumMatches([440, 440], [440]), 1);
assert.equal(maximumMatches([440, 450], [445, 440]), 2);
assert.equal(maximumMatches([], [440]), 0);
const results = [];
const total = empty();
const totalSeparated = empty();
const started = performance.now();
for (const pair of pairs) {
  const sources = pair.map(readClip);
  const mixed = new Float32Array(Math.round(duration * sampleRate));
  for (let sample = 0; sample < mixed.length; sample++) mixed[sample] = 0.5 * sources[0].samples[sample] + 0.5 * sources[1].samples[sample];
  const frames = []; const onsets = [];
  const before = performance.now();
  let notes;
  tf.engine().startScope();
  try {
    await basicPitch.evaluateModel(mixed, (f, o) => { frames.push(...f); onsets.push(...o); }, () => {});
    notes = noteFramesToTime(outputToNotesPoly(frames, onsets, onsetThreshold, frameThreshold, minimumNoteFrames));
  } finally { tf.engine().endScope(); }
  const inferenceSeconds = (performance.now() - before) / 1000;
  const counts = empty(); const separated = empty();
  let unisonFrames = 0;
  for (let frame = 0; frame < Math.round(duration / hop); frame++) {
    const time = frame * hop;
    const reference = sources.map(source => f0At(source.f0, time)).filter(frequency => frequency > 0);
    const midi = [...new Set(notes.filter(note => note.startTimeSeconds <= time && note.startTimeSeconds + note.durationSeconds > time).map(note => note.pitchMidi))];
    const predicted = midi.map(note => 440 * 2 ** ((note - 69) / 12));
    addFrame(counts, reference, predicted);
    if (reference.length === 2) {
      const distance = Math.abs(1200 * Math.log2(reference[0] / reference[1]));
      if (distance > 100) addFrame(separated, reference, predicted);
      else unisonFrames++;
    }
  }
  for (const key of Object.keys(total)) { total[key] += counts[key]; totalSeparated[key] += separated[key]; }
  const result = { pair: pair.map(id => `vocadito_${id}`).join('+'), sources: sources.map(source => source.source), durationSeconds: duration, mixtureSha256: hash(new Uint8Array(mixed.buffer)), inferenceSeconds, realTimeFactor: inferenceSeconds / duration, outputFrames: frames.length, predictedNotes: notes.length, unisonOrNearUnisonFrames: unisonFrames, counts, metrics: metrics(counts), separatedTwoVoiceCounts: separated, separatedTwoVoiceMetrics: metrics(separated), notes };
  results.push(result);
  console.error(`${result.pair}: ${inferenceSeconds.toFixed(1)}s inference / ${duration}s audio; P=${result.metrics.precision.toFixed(2)} R=${result.metrics.recall.toFixed(2)} F1=${result.metrics.f1.toFixed(2)}`);
}
const result = {
  schemaVersion: 1,
  createdAt: new Date().toISOString(),
  scope: `${pairs.length} fixed artificial two-voice mixtures; first ${duration}s per source; NOT a natural choir benchmark`,
  sources: { dataset: 'Vocadito, Zenodo 5578807 v3', license: 'CC-BY-4.0', archiveMd5: 'dea40fd18f14d899643c4ba221b33a46', model: 'spotify/basic-pitch-ts', modelProvenance: JSON.parse(readFileSync(join(modelDir, 'PROVENANCE.json'), 'utf8')) },
  modelHashes: [{ file: 'model.json', sha256: hash(modelBytes) }, ...weightPaths.map((file, index) => ({ file, sha256: hash(weightBuffers[index]) }))],
  versions: { node: process.version, tfjs: tf.version.tfjs, basicPitch: basicPitchVersion, backend: tf.getBackend(), platform: `${process.platform}/${process.arch}`, ffmpeg: execFileSync('ffmpeg', ['-version'], { encoding: 'utf8' }).split('\n')[0] },
  productionSourceHashes: { worker: hash(workerBytes), resampler: hash(resamplerBytes) },
  scriptSha256: hash(readFileSync(new URL(import.meta.url))),
  protocol: { mixture: '0.5*a+0.5*b at original start times; ffmpeg decode preserving original rate/channels, then application resampleMono box-filter to mono 22050Hz; no per-source peak normalization, shifts, pitch changes or persisted mixture audio', groundTruth: 'published continuous F0 CSV, nearest timestamp to common 10ms grid, no timing shift; two sources retained even in unison', predictions: 'active notes from noteFramesToTime(outputToNotesPoly(frames,onsets,0.5,0.3,5)); frequency at integer MIDI centre; unique active pitches; pitch bends NOT included', matching: 'maximum-cardinality 1:1 bipartite matching within 50 cents at each frame; no octave equivalence', separatedSubset: 'frames with both sources voiced and their reference F0 separated by more than 100 cents', onsetThreshold, frameThreshold, minimumNoteFrames, gridSeconds: hop, toleranceCents: 50 },
  durationSeconds: duration * pairs.length,
  elapsedSeconds: (performance.now() - started) / 1000,
  counts: total,
  microMetrics: metrics(total),
  separatedTwoVoiceCounts: totalSeparated,
  separatedTwoVoiceMetrics: metrics(totalSeparated),
  results,
};
const json = JSON.stringify(result, null, 2) + '\n';
const output = option('--output');
if (output) writeFileSync(resolve(output), json); else process.stdout.write(json);
graphModel.dispose();
console.error(`Completed: ${JSON.stringify(result.microMetrics)}`);
