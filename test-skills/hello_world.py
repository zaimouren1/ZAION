import sys, json
data = json.loads(sys.argv[1]) if len(sys.argv) > 1 else {}
name = data.get('name', 'World')
print(json.dumps({'greeting': f'Hello, {name}! (python)'}))
