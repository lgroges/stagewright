// Work that must happen one at a time, in order: each save starts from what the
// one before it left, so quick edits can't overwrite each other.

export class Queue {
  /** How many jobs are waiting or running. */
  pending = $state(0);
  private last: Promise<unknown> = Promise.resolve();

  /** Run `job` after the jobs before it; a failed job doesn't stop the next. */
  run<T>(job: () => Promise<T>): Promise<T> {
    this.pending++;
    const p = this.last.then(job).finally(() => this.pending--);
    this.last = p.catch(() => undefined);
    return p;
  }
}
