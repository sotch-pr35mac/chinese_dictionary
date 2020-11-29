# chinese_dictionary

### About
A searchable Chinese / English dictionary with helpful utilities.

### Features
- Search with Traditional Chinese characters, Simplified Chinese characters, pinyin with tone marks, pinyin with tone numbers, pinyin with no tones, and English. 
- Classify a string of text as either English, Pinyin, or Chinese characters. 
- Convert between Traditional and Simplified Chinese characters.
- Segment strings of Chinese characters into tokens using a dictionary-driven segmentation approach.

### Usage
Querying the dictionary
```rust
extern crate chinese_dictionary;

use chinese_dictionary::ChineseDictionary;

let dictionary = ChineseDictionary::new(); // Instantiation may take a while

// Querying the dictionary returns an `Option<Vec<&WordEntry>>`
// Read more about the WordEntry struct below
let query = "to run";
let results = dictionary.query(query).unwrap();
let first_result = results.first().unwrap();
println!("{}", first_result.simplified) // --> "执行"
```

Classifying a string of text
```rust
extern crate chinese_dictionary;

use chinese_dictionary::ChineseDictionary;
use chinese_dictionary::ClassificationResult;

let dictionary = ChineseDictionary::new(); // Instantiation may take a while

// Read more about the ClassificationResult enum below 
println!("{:?}", dictionary.classify("nihao")); // --> ClassificationResult::PY
```

Convert between Traditional and Simplified Chinese characters
```rust
extern crate chinese_dictionary;

use chinese_dictionary::ChineseDictionary;

let dictionary = ChineseDictionary::new(); // Instantiation may take a while

println!("{}", dictionary.convert_to_simplified("簡體字")); // --> "简体字"
println!("{}", dictionary.convert_to_traditional("繁体字")); // --> "繁體字"
```

Segment a string of characters
```rust
extern crate chinese_dictionary;

use chinese_dictionary::ChineseDictionary;

let dictionary = ChineseDictionary::new(); // Instantiation may take a while

println!("{:?}", dictionary.segment("今天天气不错")); // --> ["今天", "天气", "不错"]
```

#### `WordEntry` struct
```rust
extern crate chinese_dictionary;

use chinese_dictionary::WordEntry;
use chinese_dictionary::MeasureWord;

let example_measure_word = MeasureWord {
	traditional: "example_traditional".to_string(),
	simplified: "example_simplified".to_string(),
	pinyin_marks: "example_pinyin_marks".to_string(),
	pinyin_numbers: "example_pinyin_numbers".to_string(),
};

let example = WordEntry {
	traditional: "繁體字".to_string(),
	simplified: "繁体字".to_string(),
	pinyin_marks: "fán tǐ zì".to_string(),
	pinyin_numbers: "fan2 ti3 zi4".to_string(),
	english: vec!["traditional Chinese character".to_string()],
	tone_marks: vec![2 as u8, 3 as u8, 4 as u8],
	hash: 000000 as u64,
	measure_words: vec![example_measure_word],
	hsk: 6 as u8,
	word_id: 11111111 as u32,
};
```

#### `ClassificationResult` enum
The possible values for the `ClassificationResult` enum are:
- `PY`: Represents Pinyin
- `EN`: Represents English
- `ZH`: Represents Chinese
- `UN`: Represents an uncertain classification result

### License
This software is licensed under the [MIT License](https://github.com/sotch-pr35mac/chinese_dictionary/blob/master/LICENSE).

This project uses data from the [CC-CEDICT](), licensed under the [Creative Commons Attribute-Share Alike 4.0 License](https://creativecommons.org/licenses/by-sa/4.0/). This data has been [formatted](https://github.com/sotch-pr35mac/syng-dictionary-creator) to work with this project. The `.dictionary` files within the `data/` directory are licensed under the [Creative Commons Attribute-Share Alike 4.0 License](https://creativecommons.org/licenses/by-sa/4.0/).
