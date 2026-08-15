// Zaion Skill: hello_world
// Input: { "name": string }
// Output: { "greeting": string }

const input = JSON.parse(Deno.args[0] || '{}');
const name = input.name || 'World';
console.log(JSON.stringify({ greeting: `Hello, ${name}! From Zaion Skill Sandbox.` }));
