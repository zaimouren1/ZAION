// This skill attempts filesystem access
const fs = require('fs');
try {
  const files = fs.readdirSync('.');
  console.log(JSON.stringify({ files: files.slice(0, 3) }));
} catch(e) {
  console.log(JSON.stringify({ error: e.message }));
}
