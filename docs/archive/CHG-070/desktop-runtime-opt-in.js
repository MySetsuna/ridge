// Retired opt-in gate: native desktop always uses the shared Runtime.
const NATIVE_RUNTIME = !WEB_REMOTE && process.env.RIDGE_NATIVE_RUNTIME === '1';
