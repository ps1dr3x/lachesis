'use strict'

import React from 'react'
import ReactDOM from 'react-dom'
import Header from './components/Header'
import DataTable from './components/DataTable'
import 'semantic-ui-css/semantic.min.css'
import 'style/app.scss'

function App () {
  return (
    <div className='app'>
      <Header title='Lachesis UI' />
      <DataTable />
    </div>
  )
}

ReactDOM.render(<App />, document.querySelector('#root'))
