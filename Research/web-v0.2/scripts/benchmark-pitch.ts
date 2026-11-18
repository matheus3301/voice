/**
 * Offline corpus evaluation of the application's actual target-track detector.
 * Requires Node >=22.12, ffmpeg and ffprobe. No runtime or production dependencies.
 * node --experimental-strip-types scripts/benchmark-pitch.ts --dataset /private/path/vocadito --output /private/path/results.json
 */
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { readFileSync, readdirSync, statSync, writeFileSync } from 'node:fs';
import { resolve, join, basename, relative } from 'node:path';
import { ANALYSIS_RATE, ANALYSIS_HOP, analyzePitch, resampleMono } from '../src/audio/pitch.ts';

const args = process.argv.slice(2);
const option = (key: string) => { const index = args.indexOf(key); return index >= 0 ? args[index + 1] : undefined; };
const dataset = option('--dataset');
if (!dataset) throw new Error('Provide --dataset pointing to the extracted Vocadito directory.');
const root = resolve(dataset);
const output = option('--output');
const archive = option('--archive');
const archiveBytes = archive ? readFileSync(resolve(archive)) : null;
const archiveMd5 = archiveBytes ? createHash('md5').update(archiveBytes).digest('hex') : null;
if (archiveMd5 && archiveMd5 !== 'dea40fd18f14d899643c4ba221b33a46') throw new Error('Archive MD5 does not match Zenodo record 5578807.');
const limit = Number(option('--limit') ?? Infinity);
const sha256 = (bytes: Uint8Array) => createHash('sha256').update(bytes).digest('hex');
const walk = (dir: string): string[] => readdirSync(dir).flatMap(name => { const path = join(dir, name); return statSync(path).isDirectory() ? walk(path) : [path]; });
const paths = walk(root).filter(path => !path.includes('__MACOSX'));
const audioPaths = paths.filter(path => /\.wav$/i.test(path)).sort((a, b) => a.localeCompare(b, 'en', { numeric: true })).slice(0, limit);
if (!audioPaths.length) throw new Error('No WAV files found in dataset.');

interface Counts { frames: number; referenceVoiced: number; referenceUnvoiced: number; trueVoiced: number; falseAlarm: number; rawCorrect: number; chromaCorrect: number; octaveHigh: number; octaveLow: number; outOfRangeReference: number; lowRmsVoicingMiss: number }
const empty = (): Counts => ({ frames: 0, referenceVoiced: 0, referenceUnvoiced: 0, trueVoiced: 0, falseAlarm: 0, rawCorrect: 0, chromaCorrect: 0, octaveHigh: 0, octaveLow: 0, outOfRangeReference: 0, lowRmsVoicingMiss: 0 });
const percentage = (a: number, b: number) => b ? a / b * 100 : null;
const metrics = (c: Counts) => ({ rawPitchAccuracy50c: percentage(c.rawCorrect, c.referenceVoiced), rawChromaAccuracy50c: percentage(c.chromaCorrect, c.referenceVoiced), voicingRecall: percentage(c.trueVoiced, c.referenceVoiced), voicingFalseAlarm: percentage(c.falseAlarm, c.referenceUnvoiced), octaveErrorRateOfVoicedReference: percentage(c.octaveHigh + c.octaveLow, c.referenceVoiced), octaveErrorRateOfDetectedVoiced: percentage(c.octaveHigh + c.octaveLow, c.trueVoiced) });
const total = empty();
const started = performance.now();
const clips = [];
for (const audioPath of audioPaths) {
  const id = basename(audioPath, '.wav');
  const matches = paths.filter(path => /\.csv$/i.test(path) && basename(path).toLowerCase() === `${id.toLowerCase()}_f0.csv`);
  if (matches.length !== 1) throw new Error(`Expected one F0 CSV for ${id}; got ${matches.join(', ')}.`);
  const referencePath = matches[0];
  const rows = readFileSync(referencePath, 'utf8').trim().split(/\r?\n/).map(line => line.trim().split(/[,\t ]+/).map(Number)).filter(row => row.length >= 2 && Number.isFinite(row[0]) && Number.isFinite(row[1]));
  if (!rows.length) throw new Error(`No F0 data in ${referencePath}.`);
  const meta = JSON.parse(execFileSync('ffprobe', ['-v', 'error', '-select_streams', 'a:0', '-show_entries', 'stream=sample_rate,channels', '-of', 'json', audioPath], { encoding: 'utf8' })).streams[0];
  const sourceRate = Number(meta.sample_rate);
  const channelCount = Number(meta.channels);
  const pcm = execFileSync('ffmpeg', ['-v', 'error', '-i', audioPath, '-map', '0:a:0', '-f', 'f32le', '-acodec', 'pcm_f32le', 'pipe:1'], { maxBuffer: 256 * 1024 * 1024 });
  const sampleCount = pcm.length / 4 / channelCount;
  const channels = Array.from({ length: channelCount }, () => new Float32Array(sampleCount));
  for (let sample = 0; sample < sampleCount; sample++) for (let channel = 0; channel < channelCount; channel++) channels[channel][sample] = pcm.readFloatLE((sample * channelCount + channel) * 4);
  const samples = resampleMono(channels, sourceRate);
  const before = performance.now();
  const predictions = analyzePitch(samples, ANALYSIS_RATE);
  const analysisMilliseconds = performance.now() - before;
  const counts = empty();
  const absoluteErrors: number[] = [];
  const grossErrors: { time: number; reference: number; estimate: number; cents: number }[] = [];
  let predictionIndex = 0;
  for (const [time, reference] of rows) {
    if (time < 0 || time >= sampleCount / sourceRate) continue;
    while (predictionIndex + 1 < predictions.length && Math.abs(predictions[predictionIndex + 1].time - time) < Math.abs(predictions[predictionIndex].time - time)) predictionIndex++;
    const prediction = predictions[predictionIndex];
    // No timing optimization, oracle voicing or reference-frequency quantization.
    const estimate = prediction?.frequency ?? 0;
    const predictedVoiced = estimate > 0 && (prediction?.confidence ?? 0) >= 0.82;
    counts.frames++;
    if (reference > 0) {
      counts.referenceVoiced++;
      if (reference < 65 || reference > 1200) counts.outOfRangeReference++;
      if (!predictedVoiced && (prediction?.rms ?? 0) < 0.006) counts.lowRmsVoicingMiss++;
      if (predictedVoiced) {
        counts.trueVoiced++;
        const cents = 1200 * Math.log2(estimate / reference);
        const raw = Math.abs(cents) <= 50;
        const octave = Math.round(cents / 1200);
        const chroma = Math.abs(cents - octave * 1200) <= 50;
        if (raw) counts.rawCorrect++;
        if (chroma) counts.chromaCorrect++;
        if (!raw && chroma) { if (octave > 0) counts.octaveHigh++; else counts.octaveLow++; }
        absoluteErrors.push(Math.abs(cents));
        if (Math.abs(cents) > 100 && grossErrors.length < 6 && (!grossErrors.length || time - grossErrors[grossErrors.length - 1].time > 0.2)) grossErrors.push({ time, reference, estimate, cents });
      }
    } else { counts.referenceUnvoiced++; if (predictedVoiced) counts.falseAlarm++; }
  }
  for (const key of Object.keys(total) as (keyof Counts)[]) total[key] += counts[key];
  absoluteErrors.sort((a, b) => a - b);
  const result = { id, audio: relative(root, audioPath), annotation: relative(root, referencePath), audioSha256: sha256(readFileSync(audioPath)), annotationSha256: sha256(readFileSync(referencePath)), durationSeconds: sampleCount / sourceRate, sourceRate, channelCount, predictedFrames: predictions.length, analysisMilliseconds, counts, metrics: metrics(counts), medianAbsoluteCentsDetected: absoluteErrors.length ? absoluteErrors[Math.floor(absoluteErrors.length / 2)] : null, grossErrorExamples: grossErrors };
  clips.push(result);
  console.error(`${id}: RPA=${result.metrics.rawPitchAccuracy50c?.toFixed(2)}% RCA=${result.metrics.rawChromaAccuracy50c?.toFixed(2)}% VR=${result.metrics.voicingRecall?.toFixed(2)}% VFA=${result.metrics.voicingFalseAlarm?.toFixed(2)}%`);
}
const result = {
  schemaVersion: 1,
  createdAt: new Date().toISOString(),
  dataset: { name: 'Vocadito', zenodoRecord: '5578807', license: 'CC-BY-4.0', expectedArchiveMd5: 'dea40fd18f14d899643c4ba221b33a46', actualArchiveMd5: archiveMd5, archiveSha256: archiveBytes ? sha256(archiveBytes) : null, scope: clips.length === 40 ? 'entire 40-clip corpus' : `${clips.length}-clip subset` },
  detector: { source: 'src/audio/pitch.ts', sourceSha256: sha256(readFileSync(new URL('../src/audio/pitch.ts', import.meta.url))), gitHead: execFileSync('git', ['rev-parse', 'HEAD'], { encoding: 'utf8' }).trim(), sampleRate: ANALYSIS_RATE, hopSeconds: ANALYSIS_HOP, windowSamples: 1024, minimumFrequency: 65, maximumFrequency: 1200, confidenceThreshold: 0.82 },
  runnerSourceSha256: sha256(readFileSync(new URL(import.meta.url))),
  environment: { node: process.version, platform: `${process.platform}/${process.arch}`, ffmpeg: execFileSync('ffmpeg', ['-version'], { encoding: 'utf8' }).split('\n')[0] },
  evaluation: { grid: 'all reference annotation timestamps inside source audio duration', alignment: 'nearest predicted frame by timestamp; ties choose earlier; no timing shift optimization', voicing: 'reference f0 > 0; predicted f0 > 0 and confidence >=0.82', toleranceCents: 50, aggregation: 'micro: pooled frame counts, including detector unvoiced as incorrect on reference voiced; macro: unweighted mean of clip metrics', rawPitchFormula: 'abs(1200*log2(estimate/reference)) <= 50', rawChromaFormula: 'abs(cents - 1200*round(cents/1200)) <= 50', octaveError: 'RCA correct and RPA incorrect; sign from round(cents/1200)' },
  durationSeconds: clips.reduce((sum, clip) => sum + clip.durationSeconds, 0),
  elapsedSeconds: (performance.now() - started) / 1000,
  counts: total,
  microMetrics: metrics(total),
  macroMetrics: Object.fromEntries(Object.keys(metrics(total)).map(key => { const values = clips.map(clip => clip.metrics[key as keyof typeof clip.metrics]).filter((value): value is number => value !== null); return [key, values.length ? values.reduce((sum, value) => sum + value, 0) / values.length : null]; })),
  clips,
};
const json = JSON.stringify(result, null, 2) + '\n';
if (output) writeFileSync(resolve(output), json);
else process.stdout.write(json);
console.error(`Completed ${clips.length} clips, ${result.durationSeconds.toFixed(1)} s of singing in ${result.elapsedSeconds.toFixed(1)} s. Micro metrics: ${JSON.stringify(result.microMetrics)}`);
