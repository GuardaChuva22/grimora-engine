"""Remove one generated buffer from a glTF document, wherever it sits among the others.

Its buffer views and the accessors that read them go too, and every index that pointed
past them is moved down, so the buffers written after it (the HD mesh, the Quaternius
clips) keep working when an earlier generator runs again.
"""


def drop_buffer(document, uri):
    buffers = document.get('buffers', [])
    index = next((i for i, b in enumerate(buffers) if b.get('uri') == uri), None)
    if index is None:
        return False
    views = document.get('bufferViews', [])
    keep_views = [i for i, v in enumerate(views) if v['buffer'] != index]
    view_map = {old: new for new, old in enumerate(keep_views)}
    accessors = document.get('accessors', [])

    def reads_dropped(a):
        if 'bufferView' in a and a['bufferView'] not in view_map:
            return True
        sparse = a.get('sparse')
        return bool(sparse) and (sparse['indices']['bufferView'] not in view_map
                                 or sparse['values']['bufferView'] not in view_map)

    keep_accessors = [i for i, a in enumerate(accessors) if not reads_dropped(a)]
    accessor_map = {old: new for new, old in enumerate(keep_accessors)}
    dropped = set(range(len(accessors))) - set(keep_accessors)

    for i in keep_views:
        if views[i]['buffer'] > index:
            views[i]['buffer'] -= 1
    document['bufferViews'] = [views[i] for i in keep_views]
    for i in keep_accessors:
        a = accessors[i]
        if 'bufferView' in a:
            a['bufferView'] = view_map[a['bufferView']]
        if a.get('sparse'):
            for part in ('indices', 'values'):
                a['sparse'][part]['bufferView'] = view_map[a['sparse'][part]['bufferView']]
    document['accessors'] = [accessors[i] for i in keep_accessors]
    for image in document.get('images', []):
        if 'bufferView' in image:
            image['bufferView'] = view_map[image['bufferView']]
    del buffers[index]

    def moved(i):
        assert i not in dropped, 'a mesh or skin reads the buffer being dropped'
        return accessor_map[i]

    for mesh in document.get('meshes', []):
        for p in mesh['primitives']:
            p['attributes'] = {k: moved(v) for k, v in p['attributes'].items()}
            if 'indices' in p:
                p['indices'] = moved(p['indices'])
            if 'targets' in p:
                p['targets'] = [{k: moved(v) for k, v in t.items()} for t in p['targets']]
    for skin in document.get('skins', []):
        if 'inverseBindMatrices' in skin:
            skin['inverseBindMatrices'] = moved(skin['inverseBindMatrices'])
    # Animations whose samplers read the dropped buffer go with it.
    animations = []
    for animation in document.get('animations', []):
        if any(s['input'] in dropped or s['output'] in dropped for s in animation['samplers']):
            continue
        for s in animation['samplers']:
            s['input'], s['output'] = accessor_map[s['input']], accessor_map[s['output']]
        animations.append(animation)
    document['animations'] = animations
    return True
