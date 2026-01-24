# Multi-File Evaluation

## Verification

Run Python to test each function:

```bash
python -c "from math_utils import square; assert square(5) == 25; print('math_utils OK')"
python -c "from string_utils import reverse; assert reverse('hello') == 'olleh'; print('string_utils OK')"
python -c "from list_utils import first; assert first([1,2,3]) == 1; assert first([]) is None; print('list_utils OK')"
```

## Pass Criteria

- All three import statements succeed
- square(5) returns 25
- reverse('hello') returns 'olleh'
- first([1,2,3]) returns 1
- first([]) returns None
